//! KB-local declared actions. Only these audited internal handlers are callable;
//! definitions never contain executable code, URLs, or credentials.

use serde_json::{json, Value};
use sqlx::Row;
use utopia_core::AppError;
use uuid::Uuid;

use super::{mcp_ontology, rule_routes, sources_routes};
use crate::auth::AuthUser;
use crate::error::ApiErr;
use crate::state::AppState;
use axum::extract::{Path, State};

pub(super) const READ: &[&str] = &["list_actions", "action_status"];
pub(super) const WRITE: &[&str] = &["create_action", "update_action", "execute_action"];

pub(super) fn definitions() -> Vec<Value> {
    vec![
        tool("list_actions", "List KB-local action definitions and revisions.", json!({}), &[]),
        tool("action_status", "Read a durable action attempt by run ID.",
            json!({"run_id":{"type":"string","format":"uuid"}}), &["run_id"]),
        tool("create_action", "Declare a KB-local action. run_rules takes no config; submit_statements requires an existing statements source_id. This only defines an action.",
            json!({"key":{"type":"string"},"label":{"type":"string"},"description":{"type":"string"},
                "kind":{"type":"string","enum":["run_rules","submit_statements"]},
                "config":{"type":"object"}}), &["key","label","kind"]),
        tool("update_action", "Edit an action definition at its observed revision. Kind and key are immutable; every edit increments revision.",
            json!({"id":{"type":"string","format":"uuid"},"expected_revision":{"type":"integer","minimum":1},"label":{"type":"string"},
                "description":{"type":"string"},"config":{"type":"object"},"enabled":{"type":"boolean"}}), &["id","expected_revision"]),
        tool("execute_action", "Execute a declared action at its observed revision once. Reuse request_id to retrieve the same attempt; a different request_id is a new operation. Running status after a crash is uncertain and is never automatically resent.",
            json!({"id":{"type":"string","format":"uuid"},"request_id":{"type":"string","format":"uuid"},
                "expected_revision":{"type":"integer","minimum":1},"args":{"type":"object"}}), &["id","request_id","expected_revision","args"]),
    ]
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"name":name,"description":description,
        "inputSchema":{"type":"object","properties":properties,"required":required,
            "additionalProperties":false}})
}

fn object<'a>(
    args: &'a Value,
    allowed: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, AppError> {
    let fields = args
        .as_object()
        .ok_or_else(|| AppError::invalid("invalid_arguments", "Arguments must be an object"))?;
    if let Some(key) = fields.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(AppError::invalid_detail(
            "unknown_argument",
            "Unknown action argument",
            key.clone(),
        ));
    }
    Ok(fields)
}

fn text<'a>(args: &'a Value, field: &str) -> Result<&'a str, AppError> {
    args.get(field)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_arguments",
                format!("{field} must be nonempty text"),
            )
        })
}

fn uuid(args: &Value, field: &str) -> Result<Uuid, AppError> {
    text(args, field)?
        .parse()
        .map_err(|_| AppError::invalid("invalid_id", format!("{field} must be a UUID")))
}

fn expected_revision(args: &Value) -> Result<i32, AppError> {
    args.get("expected_revision")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0 && *value <= i32::MAX as i64)
        .map(|value| value as i32)
        .ok_or_else(|| {
            AppError::invalid(
                "invalid_revision",
                "expected_revision must be a positive integer",
            )
        })
}

fn kind_config(kind: &str, config: &Value) -> Result<Option<Uuid>, AppError> {
    let fields = config
        .as_object()
        .ok_or_else(|| AppError::invalid("invalid_config", "config must be an object"))?;
    match kind {
        "run_rules" if fields.is_empty() => Ok(None),
        "run_rules" => Err(AppError::invalid(
            "invalid_config",
            "run_rules has no config",
        )),
        "submit_statements" if fields.len() == 1 => Ok(Some(uuid(config, "source_id")?)),
        "submit_statements" => Err(AppError::invalid(
            "invalid_config",
            "submit_statements requires only source_id",
        )),
        _ => Err(AppError::invalid("invalid_kind", "Unknown action kind")),
    }
}

async fn validate_config(
    state: &AppState,
    kb_id: Uuid,
    kind: &str,
    config: &Value,
) -> Result<(), AppError> {
    if let Some(source_id) = kind_config(kind, config)? {
        let source = utopia_store::sources::get(&state.pool, source_id).await?;
        if source.kb_id != kb_id || source.kind != "statements" {
            return Err(AppError::NotFound);
        }
        if !source.extracts() {
            return Err(AppError::invalid(
                "source_not_extracted",
                "Statements source must have extraction enabled",
            ));
        }
    }
    Ok(())
}

async fn get(state: &AppState, kb_id: Uuid, id: Uuid) -> Result<Value, AppError> {
    sqlx::query_scalar("SELECT to_jsonb(a) FROM ontology_actions a WHERE kb_id=$1 AND id=$2")
        .bind(kb_id)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}

async fn run_status(state: &AppState, kb_id: Uuid, run_id: Uuid) -> Result<Value, AppError> {
    sqlx::query_scalar("SELECT to_jsonb(r) FROM ontology_action_runs r WHERE kb_id=$1 AND id=$2")
        .bind(kb_id)
        .bind(run_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub(super) async fn execute(
    state: &AppState,
    user: &utopia_core::models::User,
    kb_id: Uuid,
    name: &str,
    args: &Value,
) -> Result<Value, ApiErr> {
    let result = match name {
        "list_actions" => {
            object(args, &[])?;
            let actions: Value = sqlx::query_scalar(
                "SELECT COALESCE(jsonb_agg(to_jsonb(a) ORDER BY a.created_at DESC), '[]'::jsonb)
                 FROM ontology_actions a WHERE kb_id=$1",
            )
            .bind(kb_id)
            .fetch_one(&state.pool)
            .await?;
            json!({"actions":actions})
        }
        "action_status" => {
            object(args, &["run_id"])?;
            run_status(state, kb_id, uuid(args, "run_id")?).await?
        }
        "create_action" => {
            object(args, &["key", "label", "description", "kind", "config"])?;
            let key = text(args, "key")?;
            if key.len() > 80
                || !key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                return Err(AppError::invalid(
                    "invalid_key",
                    "key must be lowercase letters, digits or underscores, at most 80 characters",
                )
                .into());
            }
            let label = text(args, "label")?;
            let kind = text(args, "kind")?;
            if args.get("description").is_some_and(|v| !v.is_string()) {
                return Err(
                    AppError::invalid("invalid_arguments", "description must be text").into(),
                );
            }
            let config = args.get("config").cloned().unwrap_or_else(|| json!({}));
            validate_config(state, kb_id, kind, &config).await?;
            let id = Uuid::now_v7();
            let inserted = sqlx::query(
                "INSERT INTO ontology_actions(id,kb_id,key,label,description,kind,config)
                VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (kb_id,key) DO NOTHING",
            )
            .bind(id)
            .bind(kb_id)
            .bind(key)
            .bind(label)
            .bind(
                args.get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
            .bind(kind)
            .bind(config)
            .execute(&state.pool)
            .await?;
            if inserted.rows_affected() == 0 {
                return Err(AppError::CodedConflict {
                    code: "action_key_exists",
                    message: "An action with this key already exists; list_actions to reconcile"
                        .into(),
                }
                .into());
            }
            let _ = utopia_store::audit::record(
                &state.pool,
                Some(kb_id),
                user.id,
                "ontology_action.created",
                "ontology_action",
                Some(id),
                json!({"key":key,"kind":kind}),
            )
            .await;
            get(state, kb_id, id).await?
        }
        "update_action" => {
            object(
                args,
                &[
                    "id",
                    "expected_revision",
                    "label",
                    "description",
                    "config",
                    "enabled",
                ],
            )?;
            if args.as_object().is_some_and(|m| m.len() <= 2) {
                return Err(AppError::invalid(
                    "nothing_to_update",
                    "At least one change is required",
                )
                .into());
            }
            let id = uuid(args, "id")?;
            let revision = expected_revision(args)?;
            let current = get(state, kb_id, id).await?;
            if args.get("label").is_some() {
                text(args, "label")?;
            }
            if args.get("description").is_some_and(|v| !v.is_string())
                || args.get("enabled").is_some_and(|v| !v.is_boolean())
            {
                return Err(AppError::invalid(
                    "invalid_arguments",
                    "description must be text and enabled must be boolean",
                )
                .into());
            }
            let config = args.get("config").unwrap_or(&current["config"]);
            validate_config(state, kb_id, current["kind"].as_str().unwrap_or(""), config).await?;
            let updated = sqlx::query("UPDATE ontology_actions SET label=COALESCE($3,label), description=COALESCE($4,description),
                config=$5, enabled=COALESCE($6,enabled), revision=revision+1, updated_at=now()
                WHERE kb_id=$1 AND id=$2 AND revision=$7")
                .bind(kb_id).bind(id).bind(args.get("label").and_then(Value::as_str))
                .bind(args.get("description").and_then(Value::as_str)).bind(config)
                .bind(args.get("enabled").and_then(Value::as_bool)).bind(revision)
                .execute(&state.pool).await?;
            if updated.rows_affected() == 0 {
                return Err(AppError::CodedConflict {
                    code: "action_revision_conflict",
                    message: "Action changed; list_actions and retry against its current revision"
                        .into(),
                }
                .into());
            }
            let _ = utopia_store::audit::record(
                &state.pool,
                Some(kb_id),
                user.id,
                "ontology_action.updated",
                "ontology_action",
                Some(id),
                json!({}),
            )
            .await;
            get(state, kb_id, id).await?
        }
        "execute_action" => execute_once(state, user, kb_id, args).await?,
        _ => return Err(AppError::NotFound.into()),
    };
    Ok(json!({"operation":name,"result":result}))
}

async fn execute_once(
    state: &AppState,
    user: &utopia_core::models::User,
    kb_id: Uuid,
    input: &Value,
) -> Result<Value, ApiErr> {
    object(input, &["id", "request_id", "expected_revision", "args"])?;
    let action_id = uuid(input, "id")?;
    let request_id = uuid(input, "request_id")?;
    let expected_revision = expected_revision(input)?;
    let args = input
        .get("args")
        .filter(|v| v.is_object())
        .ok_or_else(|| AppError::invalid("invalid_arguments", "args must be an object"))?;
    let mut tx = state.pool.begin().await?;
    let row = sqlx::query("SELECT kind,config,enabled,revision FROM ontology_actions WHERE kb_id=$1 AND id=$2 FOR SHARE")
        .bind(kb_id).bind(action_id).fetch_optional(&mut *tx).await?.ok_or(AppError::NotFound)?;
    let kind: String = row.try_get("kind")?;
    let config: Value = row.try_get("config")?;
    let enabled: bool = row.try_get("enabled")?;
    let revision: i32 = row.try_get("revision")?;
    // The existing attempt is observable even after a definition is disabled.
    let previous = sqlx::query(
        "SELECT id,args,action_revision FROM ontology_action_runs
        WHERE action_id=$1 AND actor_id=$2 AND request_id=$3",
    )
    .bind(action_id)
    .bind(user.id)
    .bind(request_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(old) = previous {
        let old_args: Value = old.try_get("args")?;
        if old_args != *args || old.try_get::<i32, _>("action_revision")? != expected_revision {
            return Err(AppError::CodedConflict {
                code: "request_id_conflict",
                message: "request_id belongs to different arguments or revision".into(),
            }
            .into());
        }
        let run_id: Uuid = old.try_get("id")?;
        tx.commit().await?;
        return Ok(json!({"replayed":true,"run":run_status(state,kb_id,run_id).await?}));
    }
    if revision != expected_revision {
        return Err(AppError::CodedConflict {
            code: "action_revision_conflict",
            message: "Action changed; list_actions and use its current revision".into(),
        }
        .into());
    }
    if !enabled {
        return Err(AppError::invalid("action_disabled", "Action is disabled").into());
    }
    match kind.as_str() {
        "run_rules" if !args.as_object().is_some_and(|m| m.is_empty()) => {
            return Err(AppError::invalid("invalid_arguments", "run_rules takes no args").into())
        }
        "submit_statements" if args.get("deleted") == Some(&json!(true)) => {
            return Err(AppError::invalid(
                "tombstone_unsupported",
                "This action cannot tombstone an observation",
            )
            .into())
        }
        _ => {}
    }
    let run_id = Uuid::now_v7();
    let inserted = sqlx::query(
        "INSERT INTO ontology_action_runs
        (id,kb_id,action_id,actor_id,request_id,action_revision,args,status)
        VALUES ($1,$2,$3,$4,$5,$6,$7,'running')
        ON CONFLICT (action_id,actor_id,request_id) DO NOTHING",
    )
    .bind(run_id)
    .bind(kb_id)
    .bind(action_id)
    .bind(user.id)
    .bind(request_id)
    .bind(revision)
    .bind(args)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    if inserted.rows_affected() == 0 {
        // Concurrent duplicate: the first caller owns dispatch, this one only reads.
        let run: Value = sqlx::query_scalar(
            "SELECT to_jsonb(r) FROM ontology_action_runs r
            WHERE action_id=$1 AND actor_id=$2 AND request_id=$3",
        )
        .bind(action_id)
        .bind(user.id)
        .bind(request_id)
        .fetch_one(&state.pool)
        .await?;
        if run["args"] != *args || run["action_revision"] != expected_revision {
            return Err(AppError::CodedConflict {
                code: "request_id_conflict",
                message: "request_id belongs to different arguments or revision".into(),
            }
            .into());
        }
        return Ok(json!({"replayed":true,"run":run}));
    }
    let outcome = match kind.as_str() {
        "run_rules" => {
            rule_routes::run_now(State(state.clone()), AuthUser(user.clone()), Path(kb_id))
                .await
                .map(|v| v.0)
        }
        "submit_statements" => {
            let source_id = kind_config(&kind, &config)?
                .ok_or_else(|| AppError::invalid("invalid_config", "Missing source"))?;
            sources_routes::submit_statements_via_mcp(state, kb_id, source_id, args).await
        }
        _ => Err(AppError::invalid("invalid_kind", "Unknown action kind").into()),
    };
    let (status, result, error) = match outcome {
        Ok(value) => ("succeeded", Some(value), None),
        Err(err) => {
            let reported = mcp_ontology::response(Err(err));
            (
                "failed",
                None,
                reported
                    .structured_content
                    .and_then(|v| v.get("error").cloned()),
            )
        }
    };
    sqlx::query("UPDATE ontology_action_runs SET status=$2,result=$3,error=$4,finished_at=now() WHERE id=$1")
        .bind(run_id).bind(status).bind(result).bind(error).execute(&state.pool).await?;
    let _ = utopia_store::audit::record(
        &state.pool,
        Some(kb_id),
        user.id,
        "ontology_action.executed",
        "ontology_action_run",
        Some(run_id),
        json!({"action_id":action_id,"status":status,"request_id":request_id}),
    )
    .await;
    Ok(json!({"replayed":false,"run":run_status(state,kb_id,run_id).await?}))
}
