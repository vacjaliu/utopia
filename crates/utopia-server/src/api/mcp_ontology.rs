//! MCP-only ontology editor tools. The handlers below call the same HTTP route
//! functions as the workbench, so authorization, audit, indexing and rule
//! validation do not acquire a second implementation.

use axum::extract::{Path, State};
use axum::Json;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use utopia_core::models::User;
use utopia_core::AppError;
use uuid::Uuid;

use super::{ontology_routes, rule_routes, sources_routes};
use crate::auth::AuthUser;
use crate::error::ApiErr;
use crate::state::AppState;

pub(super) const READ: &[&str] = &[
    "list_ontology",
    "list_rule_definitions",
    "rule_versions",
    "list_statement_sources",
    "statement_status",
];
pub(super) const WRITE: &[&str] = &[
    "create_entity_type",
    "update_entity_type",
    "create_relation_type",
    "update_relation_type",
    "create_rule",
    "update_rule",
    "run_rules",
    "submit_statements",
];

/// The workbench routes accept form payloads. MCP deliberately rejects unknown
/// fields: an agent must not mistake a misspelled axiom for a successful write.
fn parse<T: DeserializeOwned>(args: &Value, allowed: &[&str]) -> Result<T, AppError> {
    let object = args.as_object().ok_or_else(|| {
        AppError::invalid("invalid_arguments", "Tool arguments must be an object")
    })?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(AppError::invalid_detail(
            "unknown_argument",
            "Unknown tool argument",
            key.clone(),
        ));
    }
    serde_json::from_value(args.clone()).map_err(|err| {
        AppError::invalid_detail(
            "invalid_arguments",
            "Invalid tool arguments",
            err.to_string(),
        )
    })
}

fn required_text(value: &str, field: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::invalid_detail(
            "empty_argument",
            "Required text cannot be empty",
            field,
        ));
    }
    Ok(())
}

fn id(args: &Value) -> Result<Uuid, AppError> {
    uuid_arg(args, "id")
}

fn uuid_arg(args: &Value, field: &str) -> Result<Uuid, AppError> {
    args.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::invalid("id_required", format!("{field} is required")))?
        .parse()
        .map_err(|_| AppError::invalid("invalid_id", format!("{field} must be a UUID")))
}

fn merge_view(view: Value, patch: &Value, allowed: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    if let Some(fields) = view.as_object() {
        for (key, value) in fields {
            if allowed.contains(&key.as_str()) {
                result.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(fields) = patch.as_object() {
        for (key, value) in fields {
            if key != "id" {
                result.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(result)
}

fn require_patch(args: &Value) -> Result<(), AppError> {
    if args.as_object().is_some_and(|fields| fields.len() > 1) {
        Ok(())
    } else {
        Err(AppError::invalid(
            "nothing_to_update",
            "At least one field besides id is required",
        ))
    }
}

fn unique_refs(ids: &[Uuid]) -> Result<(), AppError> {
    if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
        Err(AppError::invalid(
            "duplicate_reference",
            "Reference lists must not contain duplicate IDs",
        ))
    } else {
        Ok(())
    }
}

fn validate_conditions_shape(args: &Value) -> Result<(), AppError> {
    if let Some(conditions) = args.get("conditions").and_then(Value::as_array) {
        for (index, condition) in conditions.iter().enumerate() {
            let Some(fields) = condition.as_object() else {
                continue; // serde reports the wrong type below
            };
            if let Some(key) = fields.keys().find(|key| {
                !["group", "predicate_id", "op", "operand", "side"].contains(&key.as_str())
            }) {
                return Err(AppError::invalid_detail(
                    "unknown_argument",
                    "Unknown condition field",
                    format!("conditions[{index}].{key}"),
                ));
            }
        }
    }
    Ok(())
}

async fn classes_exist(state: &AppState, kb_id: Uuid, ids: &[Uuid]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM entity_types WHERE kb_id=$1 AND id=ANY($2::uuid[])",
    )
    .bind(kb_id)
    .bind(ids)
    .fetch_one(&state.pool)
    .await?;
    let distinct = ids.iter().collect::<std::collections::HashSet<_>>().len() as i64;
    if found != distinct {
        return Err(AppError::invalid(
            "unknown_class",
            "Every referenced class must exist in this base",
        ));
    }
    Ok(())
}

async fn qualifiers_exist(state: &AppState, kb_id: Uuid, ids: &[Uuid]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    let found: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM relation_types WHERE kb_id=$1 AND kind='attribute' AND id=ANY($2::uuid[])",
    )
    .bind(kb_id)
    .bind(ids)
    .fetch_one(&state.pool)
    .await?;
    let distinct = ids.iter().collect::<std::collections::HashSet<_>>().len() as i64;
    if found != distinct {
        return Err(AppError::invalid(
            "unknown_qualifier",
            "Every qualifier must be an attribute in this base",
        ));
    }
    Ok(())
}

async fn relation_refs(
    state: &AppState,
    kb_id: Uuid,
    req: &ontology_routes::RelationTypeReq,
) -> Result<(), AppError> {
    let mut classes = Vec::new();
    classes.extend(req.domains.as_deref().unwrap_or(&[]));
    classes.extend(req.ranges.as_deref().unwrap_or(&[]));
    unique_refs(req.domains.as_deref().unwrap_or(&[]))?;
    unique_refs(req.ranges.as_deref().unwrap_or(&[]))?;
    unique_refs(req.qualifiers.as_deref().unwrap_or(&[]))?;
    classes_exist(state, kb_id, &classes).await?;
    qualifiers_exist(state, kb_id, req.qualifiers.as_deref().unwrap_or(&[])).await
}

async fn statement_source(
    state: &AppState,
    kb_id: Uuid,
    source_id: Uuid,
) -> Result<utopia_core::models::Source, AppError> {
    let source = utopia_store::sources::get(&state.pool, source_id).await?;
    if source.kb_id != kb_id || source.kind != "statements" {
        return Err(AppError::NotFound);
    }
    Ok(source)
}

async fn status_for_statement(
    state: &AppState,
    kb_id: Uuid,
    source_id: Uuid,
    external_id: &str,
) -> Result<Value, AppError> {
    statement_source(state, kb_id, source_id).await?;
    let key = format!("statements:{}", external_id.trim());
    let document = utopia_store::documents::find_by_external_key(&state.pool, source_id, &key)
        .await?
        .ok_or(AppError::NotFound)?;
    let (open_facts, typed_facts): (i64, i64) = sqlx::query_as(
        "SELECT count(DISTINCT f.id) FILTER (WHERE f.layer='open' AND f.invalidated_at IS NULL),
                count(DISTINCT f.id) FILTER (WHERE f.layer='typed' AND f.invalidated_at IS NULL)
         FROM fact_evidence ev
         JOIN chunks c ON c.id=ev.chunk_id AND c.document_id=$1 AND c.superseded_at IS NULL
         JOIN facts f ON f.id=ev.fact_id
         WHERE ev.document_id=$1 AND f.kb_id=$2",
    )
    .bind(document.id)
    .bind(kb_id)
    .fetch_one(&state.pool)
    .await?;
    let version: i32 = sqlx::query_scalar(
        "SELECT COALESCE(max(version), 0) FROM document_versions WHERE document_id=$1",
    )
    .bind(document.id)
    .fetch_one(&state.pool)
    .await?;
    Ok(json!({
        "source_id": source_id, "external_id": external_id.trim(),
        "document_id": document.id, "version": version,
        "document_status": document.status,
        "graph_status": document.graph_status,
        "document_error": document.error, "graph_error": document.graph_error,
        "deleted": document.deleted_at.is_some(),
        "open_fact_count": open_facts, "typed_fact_count": typed_facts,
    }))
}

pub(super) fn definitions() -> Vec<Value> {
    let mut result = Vec::new();
    let mut add = |name: &str, description: &str, properties: Value, required: &[&str]| {
        result.push(json!({
            "name": name, "description": description,
            "inputSchema": {
                "type": "object", "properties": properties,
                "required": required, "additionalProperties": false
            }
        }));
    };
    add(
        "list_ontology",
        "List class and relation definitions, including IDs and usage counts.",
        json!({}),
        &[],
    );
    add("list_rule_definitions", "List executable business rule definitions as structured data, including IDs and enabled state.", json!({}), &[]);
    add(
        "rule_versions",
        "Read the definition history of one executable business rule.",
        json!({"id": {"type": "string", "format": "uuid"}}),
        &["id"],
    );
    add("list_statement_sources", "List the existing statements sources available in this base. Use one source ID to submit attributable entity and relationship statements.",
        json!({}), &[]);
    add("statement_status", "Look up one submitted observation by source and external ID. Document processing, open facts and typed facts are reported separately.",
        json!({"source_id":{"type":"string","format":"uuid"},"external_id":{"type":"string"}}),
        &["source_id","external_id"]);
    add("create_entity_type", "Create a class definition, not an entity instance. A duplicate key fails; list_ontology to reconcile a retry.",
        json!({"key": {"type":"string"}, "label": {"type":"string"},
            "description": {"type":"string"}, "color": {"type":"string"},
            "shape": {"type":"string", "enum":["circle","square"]},
            "parents": {"type":"array", "items":{"type":"string", "format":"uuid"}},
            "disjoint": {"type":"array", "items":{"type":"string", "format":"uuid"}}}),
        &["key", "label"]);
    add("update_entity_type", "Update a class definition by ID. Omitted fields retain their current values; read list_ontology to inspect them.",
        json!({"id": {"type":"string", "format":"uuid"}, "label": {"type":"string"},
            "description": {"type":"string"}, "color": {"type":"string"},
            "shape": {"type":"string", "enum":["circle","square"]},
            "parents": {"type":"array", "items":{"type":"string", "format":"uuid"}},
            "disjoint": {"type":"array", "items":{"type":"string", "format":"uuid"}}}),
        &["id"]);
    let relation = json!({"key": {"type":"string"}, "label": {"type":"string"},
        "description": {"type":"string"}, "kind": {"type":"string", "enum":["relation","attribute"]},
        "temporal": {"type":"string", "enum":["state","event","eternal"]},
        "domains": {"type":"array", "items":{"type":"string", "format":"uuid"}},
        "ranges": {"type":"array", "items":{"type":"string", "format":"uuid"}},
        "qualifiers": {"type":"array", "items":{"type":"string", "format":"uuid"}},
        "datatype": {"type":"string", "enum":["text","number","date","bool"]},
        "unit": {"type":"string"},
        "functional": {"type":"boolean"}, "inverse_functional": {"type":"boolean"},
        "is_transitive": {"type":"boolean"}, "is_symmetric": {"type":"boolean"},
        "is_asymmetric": {"type":"boolean"}, "is_irreflexive": {"type":"boolean"},
        "inverse_of": {"type":"string", "format":"uuid"},
        "sub_property_of": {"type":"string", "format":"uuid"}});
    add("create_relation_type", "Create a relation or attribute definition, not a fact. A duplicate key fails; list_ontology to reconcile a retry.",
        relation.clone(), &["key", "label", "kind"]);
    let mut update_relation = relation.as_object().expect("literal object").clone();
    update_relation.remove("key");
    update_relation.remove("kind");
    update_relation.insert("id".into(), json!({"type":"string", "format":"uuid"}));
    add("update_relation_type", "Update a relation definition by ID. Omitted fields retain their current values; read list_ontology to inspect them.",
        Value::Object(update_relation), &["id"]);
    let conclusion = json!({"name": {"type":"string"}, "description": {"type":"string"},
    "subject_type_id": {"type":"string", "format":"uuid"},
    "conclusion": {"type":"string", "enum":["typing","attribute","computed","relation"]},
    "conclude_type_id": {"type":"string", "format":"uuid"},
    "conclude_predicate_id": {"type":"string", "format":"uuid"},
    "conclude_value": {}, "conclude_expr": {},
    "join_predicate_id": {"type":"string", "format":"uuid"},
    "conditions": {"type":"array", "minItems":1, "items":{
        "type":"object", "additionalProperties":false,
        "properties": {
            "group":{"type":"integer"},
            "predicate_id":{"type":"string", "format":"uuid"},
            "op":{"type":"string", "enum":["gt","gte","lt","lte","between","in","not_in","present"]},
            "operand":{}, "side":{"type":"string", "enum":["x","y"]}
        }, "required":["predicate_id","op"]
    }}});
    add("create_rule", "Create an executable business rule definition. Conditions and conclusion are validated by the existing rule engine; this does not execute it.",
        conclusion.clone(), &["name", "subject_type_id", "conclusion", "conditions"]);
    let mut patch = conclusion.as_object().expect("literal object").clone();
    patch.remove("subject_type_id");
    patch.insert("id".into(), json!({"type":"string", "format":"uuid"}));
    patch.insert("enabled".into(), json!({"type":"boolean"}));
    add("update_rule", "Update a rule by ID. Conditions or conclusion, when supplied, replace the whole respective group.",
        Value::Object(patch), &["id"]);
    add("run_rules", "Run the existing materialization engine now for this base; returns actual insertion and cap counts.", json!({}), &[]);
    add("submit_statements", "Submit source-attributed entity and relationship statements through the existing open contract. The response confirms ingestion, not typed assertion; poll statement_status and use graph reads to verify alignment.",
        json!({"source_id":{"type":"string","format":"uuid"},"payload":{
            "type":"object", "additionalProperties":false,
            "properties":{
                "external_id":{"type":"string"},"doc_time":{"type":"string","format":"date-time"},
                "deleted":{"type":"boolean"},"e":{"type":"array"},
                "s":{"type":"array"},"n":{"type":"array"}
            }, "required":["external_id","e","s","n"]
        }}), &["source_id","payload"]);
    result
}

pub(super) async fn execute(
    state: AppState,
    user: User,
    kb_id: Uuid,
    name: &str,
    args: &Value,
) -> Result<Value, ApiErr> {
    let actor = || AuthUser(user.clone());
    let state_arg = || State(state.clone());
    let result = match name {
        "list_ontology" => {
            let _: Value = parse(args, &[])?;
            ontology_routes::get(state_arg(), actor(), Path(kb_id))
                .await?
                .0
        }
        "list_rule_definitions" => {
            let _: Value = parse(args, &[])?;
            rule_routes::list(state_arg(), actor(), Path(kb_id))
                .await?
                .0
        }
        "rule_versions" => {
            let _: Value = parse(args, &["id"])?;
            rule_routes::versions(state_arg(), actor(), Path((kb_id, id(args)?)))
                .await?
                .0
        }
        "list_statement_sources" => {
            let _: Value = parse(args, &[])?;
            let sources = utopia_store::sources::list(&state.pool, kb_id).await?;
            json!({"sources": sources.into_iter().filter(|source| source.kind == "statements")
                .map(|source| json!({"id":source.id,"name":source.name,"doc_count":source.doc_count,
                    "extracts":source.config.get("extract") != Some(&json!(false))}))
                .collect::<Vec<_>>()})
        }
        "statement_status" => {
            let _: Value = parse(args, &["source_id", "external_id"])?;
            let source_id = uuid_arg(args, "source_id")?;
            let external_id = args
                .get("external_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::invalid("external_id_required", "external_id is required")
                })?;
            required_text(external_id, "external_id")?;
            status_for_statement(&state, kb_id, source_id, external_id).await?
        }
        "create_entity_type" => {
            let req: ontology_routes::EntityTypeReq = parse(
                args,
                &[
                    "key",
                    "label",
                    "color",
                    "shape",
                    "parents",
                    "disjoint",
                    "description",
                ],
            )?;
            required_text(&req.label, "label")?;
            let mut refs = req.parents.clone();
            refs.extend(req.disjoint.as_deref().unwrap_or(&[]));
            unique_refs(&req.parents)?;
            unique_refs(req.disjoint.as_deref().unwrap_or(&[]))?;
            classes_exist(&state, kb_id, &refs).await?;
            ontology_routes::create_entity_type(state_arg(), actor(), Path(kb_id), Json(req))
                .await?
                .0
        }
        "update_entity_type" => {
            let allowed = &[
                "id",
                "label",
                "color",
                "shape",
                "parents",
                "disjoint",
                "description",
            ];
            let _: Value = parse(args, allowed)?;
            require_patch(args)?;
            let target = id(args)?;
            let view = utopia_store::ontology::entity_type_views(&state.pool, kb_id)
                .await?
                .into_iter()
                .find(|row| row.id == target)
                .ok_or(AppError::NotFound)?;
            let mut merged = merge_view(json!(view), args, allowed);
            // The HTTP handler treats omitted disjoint as "leave untouched".
            // Retain that behavior instead of rewriting the set on a label edit.
            if args.get("disjoint").is_none() {
                merged
                    .as_object_mut()
                    .expect("merged object")
                    .remove("disjoint");
            }
            let req: ontology_routes::EntityTypeReq = parse(&merged, allowed)?;
            required_text(&req.label, "label")?;
            let mut refs = req.parents.clone();
            refs.extend(req.disjoint.as_deref().unwrap_or(&[]));
            unique_refs(&req.parents)?;
            unique_refs(req.disjoint.as_deref().unwrap_or(&[]))?;
            classes_exist(&state, kb_id, &refs).await?;
            ontology_routes::update_entity_type(
                state_arg(),
                actor(),
                Path((kb_id, target)),
                Json(req),
            )
            .await?
            .0
        }
        "create_relation_type" => {
            let req: ontology_routes::RelationTypeReq = parse(
                args,
                &[
                    "key",
                    "label",
                    "temporal",
                    "functional",
                    "inverse_functional",
                    "is_transitive",
                    "is_symmetric",
                    "is_asymmetric",
                    "is_irreflexive",
                    "inverse_of",
                    "sub_property_of",
                    "description",
                    "kind",
                    "domains",
                    "ranges",
                    "qualifiers",
                    "datatype",
                    "unit",
                ],
            )?;
            required_text(&req.label, "label")?;
            if req.kind.is_none() {
                return Err(AppError::invalid("kind_required", "kind is required").into());
            }
            relation_refs(&state, kb_id, &req).await?;
            ontology_routes::create_relation_type(state_arg(), actor(), Path(kb_id), Json(req))
                .await?
                .0
        }
        "update_relation_type" => {
            let allowed = &[
                "id",
                "label",
                "temporal",
                "functional",
                "inverse_functional",
                "is_transitive",
                "is_symmetric",
                "is_asymmetric",
                "is_irreflexive",
                "inverse_of",
                "sub_property_of",
                "description",
                "domains",
                "ranges",
                "qualifiers",
                "datatype",
                "unit",
            ];
            let _: Value = parse(args, allowed)?;
            require_patch(args)?;
            let target = id(args)?;
            let view = utopia_store::ontology::relation_type_views(&state.pool, kb_id)
                .await?
                .into_iter()
                .find(|row| row.id == target)
                .ok_or(AppError::NotFound)?;
            let mut merged = merge_view(json!(view), args, allowed);
            // Domain/range and qualifier fields are optional groups in the
            // workbench route. Omit them when the caller did not edit them.
            if args.get("domains").is_none() && args.get("ranges").is_none() {
                let fields = merged.as_object_mut().expect("merged object");
                fields.remove("domains");
                fields.remove("ranges");
            }
            if args.get("qualifiers").is_none() {
                merged
                    .as_object_mut()
                    .expect("merged object")
                    .remove("qualifiers");
            }
            let req: ontology_routes::RelationTypeReq = parse(&merged, allowed)?;
            required_text(&req.label, "label")?;
            relation_refs(&state, kb_id, &req).await?;
            ontology_routes::update_relation_type(
                state_arg(),
                actor(),
                Path((kb_id, target)),
                Json(req),
            )
            .await?
            .0
        }
        "create_rule" => {
            validate_conditions_shape(args)?;
            let req: rule_routes::RuleReq = parse(
                args,
                &[
                    "name",
                    "description",
                    "subject_type_id",
                    "conclusion",
                    "conclude_type_id",
                    "conclude_predicate_id",
                    "conclude_value",
                    "conclude_expr",
                    "join_predicate_id",
                    "conditions",
                ],
            )?;
            required_text(&req.name, "name")?;
            rule_routes::create(state_arg(), actor(), Path(kb_id), Json(req))
                .await?
                .0
        }
        "update_rule" => {
            validate_conditions_shape(args)?;
            let fields = args.as_object().ok_or_else(|| {
                AppError::invalid("invalid_arguments", "Tool arguments must be an object")
            })?;
            require_patch(args)?;
            if fields
                .keys()
                .any(|key| key.starts_with("conclude_") || key == "join_predicate_id")
                && !fields.contains_key("conclusion")
            {
                return Err(AppError::invalid(
                    "conclusion_required",
                    "Changing a conclusion requires its kind and complete fields",
                )
                .into());
            }
            let req: rule_routes::RulePatch = parse(
                args,
                &[
                    "id",
                    "name",
                    "description",
                    "enabled",
                    "conditions",
                    "conclusion",
                    "conclude_type_id",
                    "conclude_predicate_id",
                    "conclude_value",
                    "conclude_expr",
                    "join_predicate_id",
                ],
            )?;
            rule_routes::update(state_arg(), actor(), Path((kb_id, id(args)?)), Json(req))
                .await?
                .0
        }
        "run_rules" => {
            let _: Value = parse(args, &[])?;
            rule_routes::run_now(state_arg(), actor(), Path(kb_id))
                .await?
                .0
        }
        "submit_statements" => {
            let _: Value = parse(args, &["source_id", "payload"])?;
            let source_id = uuid_arg(args, "source_id")?;
            let payload = args
                .get("payload")
                .ok_or_else(|| AppError::invalid("payload_required", "payload is required"))?;
            if payload.get("deleted") == Some(&json!(true)) {
                return Err(AppError::invalid(
                    "tombstone_unsupported",
                    "submit_statements cannot tombstone an observation",
                )
                .into());
            }
            let pushed =
                sources_routes::submit_statements_via_mcp(&state, kb_id, source_id, payload)
                    .await?;
            let external_id = payload
                .get("external_id")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::invalid("external_id_required", "external_id is required")
                })?;
            let status = status_for_statement(&state, kb_id, source_id, external_id).await?;
            json!({"action":pushed["action"],"observation":status,
                "note":"Ingestion accepted; extraction and typed alignment may still be pending"})
        }
        _ => return Err(AppError::NotFound.into()),
    };
    Ok(json!({"operation": name, "result": result}))
}

pub(super) fn response(result: Result<Value, ApiErr>) -> super::tools::ToolResult {
    match result {
        Ok(data) => {
            super::tools::ToolResult::new(data.to_string(), json!({"kind":"tool", "status":"ok"}))
                .structured(data)
        }
        Err(ApiErr(error)) => {
            let (code, message, detail) = match error {
                AppError::Invalid {
                    code,
                    message,
                    detail,
                } => (code, message, detail),
                AppError::CodedConflict { code, message } => (code, message, None),
                AppError::Validation(message) => ("invalid_arguments", message, None),
                AppError::Conflict(message) => ("conflict", message, None),
                AppError::NotFound => ("not_found", "Not found".into(), None),
                AppError::Forbidden => ("forbidden", "Forbidden".into(), None),
                AppError::Unauthorized => ("unauthorized", "Unauthorized".into(), None),
                AppError::Db(error) => {
                    tracing::error!(%error, "MCP ontology tool database error");
                    ("internal", "Internal server error".into(), None)
                }
                AppError::Other(error) => {
                    tracing::error!(%error, "MCP ontology tool error");
                    ("internal", "Internal server error".into(), None)
                }
            };
            let data = json!({"error": {"code": code, "message": message, "detail": detail}});
            super::tools::ToolResult::new(message, json!({"kind":"tool", "status":"failed"}))
                .structured(data)
                .error()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_cover_only_known_tools_and_reject_unknown_fields() {
        let definitions = definitions();
        assert_eq!(definitions.len(), READ.len() + WRITE.len());
        assert!(definitions
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false));
        let bad: Result<Value, _> = parse(&json!({"lable":"Typo"}), &["label"]);
        assert!(matches!(
            bad,
            Err(AppError::Invalid {
                code: "unknown_argument",
                ..
            })
        ));
    }

    #[test]
    fn a_partial_edit_retains_unspecified_definition_fields() {
        let merged = merge_view(
            json!({"label":"Old", "description":"Keep", "parents":[Uuid::nil()]}),
            &json!({"id":Uuid::now_v7(), "label":"New"}),
            &["id", "label", "description", "parents"],
        );
        assert_eq!(merged["label"], "New");
        assert_eq!(merged["description"], "Keep");
        assert_eq!(merged["parents"], json!([Uuid::nil()]));
        assert!(merged.get("id").is_none());
    }

    #[test]
    fn errors_are_machine_readable_without_leaking_internal_details() {
        let invalid = response(Err(AppError::invalid_detail(
            "unknown_class",
            "Unknown class",
            "parents[0]",
        )
        .into()));
        assert!(invalid.is_error);
        let content = invalid.structured_content.unwrap();
        assert_eq!(content["error"]["code"], "unknown_class");
        assert_eq!(content["error"]["detail"], "parents[0]");
        let internal = response(Err(AppError::Other(anyhow::anyhow!("secret")).into()));
        assert_eq!(
            internal.structured_content.unwrap()["error"]["message"],
            "Internal server error"
        );
    }
}
