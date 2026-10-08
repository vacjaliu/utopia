#!/usr/bin/env bash
# Synthetic ontology definitions only. Run against a disposable knowledge base.
set -euo pipefail

: "${UTOPIA_MCP_URL:?Set UTOPIA_MCP_URL to /api/v1/kbs/<id>/mcp}"
: "${UTOPIA_TOKEN:?Set UTOPIA_TOKEN to an editor write-scoped token}"
command -v jq >/dev/null
command -v uuidgen >/dev/null

call() {
  local request response
  request=$(jq -nc --arg name "$1" --argjson arguments "$2" \
    '{jsonrpc:"2.0",id:1,method:"tools/call",params:{name:$name,arguments:$arguments}}')
  response=$(curl -fsS "$UTOPIA_MCP_URL" \
    -H "Authorization: Bearer $UTOPIA_TOKEN" \
    -H 'Content-Type: application/json' -d "$request")
  if ! jq -e '.result.isError == false' >/dev/null <<<"$response"; then
    jq . <<<"$response" >&2
    return 1
  fi
  jq -c '.result.structuredContent.result' <<<"$response"
}

suffix=$(date +%s)
args=$(jq -nc --arg key "demo_subject_$suffix" '{key:$key,label:"Demo Subject"}')
subject=$(call create_entity_type "$args")
subject_id=$(jq -r .id <<<"$subject")
args=$(jq -nc --arg key "demo_target_$suffix" '{key:$key,label:"Demo Target"}')
target=$(call create_entity_type "$args")
target_id=$(jq -r .id <<<"$target")
args=$(jq -nc --arg key "demo_score_$suffix" \
  --arg domain "$subject_id" \
  '{key:$key,label:"demo score",kind:"attribute",datatype:"number",domains:[$domain]}')
score=$(call create_relation_type "$args")
score_id=$(jq -r .id <<<"$score")
args=$(jq -nc --arg key "demo_assesses_$suffix" \
  --arg domain "$subject_id" --arg range "$target_id" \
  '{key:$key,label:"assesses",kind:"relation",domains:[$domain],ranges:[$range]}')
relation=$(call create_relation_type "$args")
args=$(jq -nc --arg name "Demo high score $suffix" \
  --arg subject "$subject_id" --arg target "$target_id" --arg score "$score_id" \
  '{name:$name,subject_type_id:$subject,conclusion:"typing",conclude_type_id:$target,
    conditions:[{predicate_id:$score,op:"gt",operand:70}]}')
rule=$(call create_rule "$args")

jq -n --arg class "$subject_id" --arg target "$target_id" \
  --arg attribute "$score_id" --arg relation "$(jq -r .id <<<"$relation")" \
  --arg rule "$(jq -r .id <<<"$rule")" \
  '{class:$class,target_class:$target,attribute:$attribute,relation:$relation,rule:$rule}'
call list_ontology '{}'
call list_rule_definitions '{}'
args=$(jq -nc --arg key "demo_refresh_$suffix" \
  '{key:$key,label:"Recompute demo rules",kind:"run_rules"}')
action=$(call create_action "$args")
action_id=$(jq -r .id <<<"$action")
action_revision=$(jq -r .revision <<<"$action")
call list_actions '{}'
request_id=$(uuidgen)
args=$(jq -nc --arg id "$action_id" --arg request "$request_id" \
  --argjson revision "$action_revision" \
  '{id:$id,request_id:$request,expected_revision:$revision,args:{}}')
run=$(call execute_action "$args")
run_id=$(jq -r .run.id <<<"$run")
args=$(jq -nc --arg id "$run_id" '{run_id:$id}')
call action_status "$args"

# Set this to an existing statements source in the same disposable base to
# exercise entity and relationship ingestion. It does not assert typed facts.
if [[ -n "${UTOPIA_STATEMENTS_SOURCE_ID:-}" ]]; then
  observation_id="demo-observation-$suffix"
  args=$(jq -nc --arg source "$UTOPIA_STATEMENTS_SOURCE_ID" \
    --arg external "$observation_id" \
    '{source_id:$source,payload:{external_id:$external,
      e:[["Demo Alice","Demo Subject",true],["Demo Target","Demo Target",true]],
      s:[[null,"Demo Alice","assesses","Demo Target",null,{},null,null],
         [null,"Demo Alice","demo score",null,"82",{},null,null]],n:[]}}')
  call submit_statements "$args"
  args=$(jq -nc --arg key "demo_observe_$suffix" \
    --arg source "$UTOPIA_STATEMENTS_SOURCE_ID" \
    '{key:$key,label:"Ingest demo observation",kind:"submit_statements",
      config:{source_id:$source}}')
  observe_action=$(call create_action "$args")
  args=$(jq -nc --arg id "$(jq -r .id <<<"$observe_action")" \
    --arg request "$(uuidgen)" --arg external "demo-action-observation-$suffix" \
    --argjson revision "$(jq -r .revision <<<"$observe_action")" \
    '{id:$id,request_id:$request,expected_revision:$revision,
      args:{external_id:$external,e:[["Demo Bob","Demo Subject",true]],
        s:[[null,"Demo Bob","assesses","Demo Target",null,{},null,null]],n:[]}}')
  call execute_action "$args"
  args=$(jq -nc --arg source "$UTOPIA_STATEMENTS_SOURCE_ID" \
    --arg external "$observation_id" '{source_id:$source,external_id:$external}')
  call statement_status "$args"
fi
