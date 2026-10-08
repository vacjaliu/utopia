# Agents over MCP

Utopia serves every knowledge base as a **Model Context Protocol** server. Any MCP client — Claude Desktop, Cursor, an agent framework, a script — can search a base, read a document, look up an entity and ask what changed, with the same permissions as the person whose token it carries. Reading needs nothing but a token. Editors can also manage ontology definitions and executable business rules with a `write` token. `remember` records a sentence whose extracted facts **wait for a person's nod** before they reach the graph.

## Get a token

Tokens belong to people, not to bases: **Account → Agents & tokens → Personal access tokens**.

| Field | Meaning |
|---|---|
| Name | For your own bookkeeping; shows in the token list and in the audit log |
| Scope | `read` (default) or `write`. Scope is a **ceiling**, not a grant: a token can never do more than its owner can. Ontology edits and `remember` need both — the `write` scope, and editor rights in that base. A viewer's `write` token still cannot edit |
| Knowledge bases | Optional. Leave empty and the token reaches every base its owner can open; pick some to narrow it |
| Expires in | 90 days by default |

The token value (`utp_pat_…`) is shown **once**, when it is issued. Revoking it takes effect on the next call.

Anything an agent does with the token is recorded in the base's Activity as the token's owner, with the tool name, so a token is never a way around the audit trail.

## The endpoint

```
POST /api/v1/kbs/{kb_id}/mcp
Authorization: Bearer utp_pat_…
Content-Type: application/json
```

One endpoint per knowledge base, speaking JSON-RPC 2.0 (MCP protocol version `2025-06-18`, stateless HTTP). The `kb_id` is in the base's URL in the browser: `/kb/{kb_id}/…`.

Three methods are served:

- `initialize` — capabilities and protocol version
- `tools/list` — the tools below, with their JSON schemas
- `tools/call` — run one

## The tools

| Tool | What it answers |
|---|---|
| `search_chunks` | Full-text + semantic search over the base's documents. Returns the six best-matching passages, each cut at 800 characters and carrying its `document_id`. Pass `as_of` to search the base as it stood at that moment — earlier versions, documents deleted since; full-text recall stays current, so hits are right but may be incomplete |
| `get_document` | The full text of one document, all sections in order, by `document_id`. Use it when a search hit is the right document but the excerpt does not carry the answer. Capped at 24,000 characters, and says so when it cuts |
| `find_entities` | Entities by (partial) name: id, type, and a disambiguator when several share a name |
| `entity_facts` | One entity's facts with validity ranges. Pass `at` (a date) to see the world as of that day; this is the tool for "who was X in 2024". Pass `as_of` (a date or an RFC3339 moment) to see the facts **as the base held them then**, before later corrections, retractions and merges — "what did we have on record before the memo arrived". The two combine: `at` for the date asked about, `as_of` for when |
| `neighbors` | The entities linked to one entity, one hop, grouped by predicate; narrow with `predicate` or `object_type`. Takes a name as well as an id |
| `timeline` | One entity's dated facts in world-time order; `since` / `until` narrow the window |
| `paths_between` | The chains of facts joining two entities, up to three hops, shortest first. With `at`, every edge must hold at that moment; with `as_of`, the chains as the base held them then |
| `changes` | What the graph learned or revised in a window of **record** time: asserted, corrected, rejected, merged. Needs no entity; use it when the question names a period, not a subject |
| `search_docs` | Utopia's own manual, for questions about how the platform works. Never the user's documents |
| `remember` | Record one sentence into the base's memory. **Needs a `write` token held by an editor**; a token without it does not see this tool in `tools/list`, and calling it anyway says why |
| `list_ontology` | List class, relation and attribute **definitions**, with IDs and usage counts |
| `list_rule_definitions` | List executable business rule definitions as structured data, including IDs and enabled state |
| `rule_versions` | Read one executable business rule's definition history |
| `list_statement_sources` | Find an existing `statements` source ID in this base without exposing its ingest key |
| `statement_status` | Read one submitted observation's document and graph processing state, plus observed open and typed fact counts |
| `create_entity_type`, `update_entity_type` | Create or replace a class definition. These do not create an entity instance |
| `create_relation_type`, `update_relation_type` | Create or replace a relation or attribute definition. These do not assert a relationship |
| `create_rule`, `update_rule` | Create or update a business rule using the existing validated rule engine. A definition does not execute when saved |
| `run_rules` | Run the existing materialization engine now and return its actual rule and insertion counts |
| `submit_statements` | Submit entity and relationship statements to an existing `statements` source using the open extraction contract. Requires an editor's `write` token; ingestion is not a typed assertion |
| `list_actions`, `action_status` | List KB-local action definitions and inspect one durable execution attempt, including its outcome or unresolved `running` state |
| `create_action`, `update_action` | Define or edit a KB-local action with one registered handler: `run_rules` or `submit_statements` |
| `execute_action` | Run a defined action once for a caller-generated UUID `request_id`; a replay reads the same attempt |

Editing and execution tools require a `write` token held by an editor. They call the existing ontology, rule and statement handlers and use their validation and audit trail. `tools/list` hides them from read-only callers. Tool results include `structuredContent`: successful writes contain an operation and a result with an ID or counts; failures set `isError: true` and include `error.code` and `error.message`. A duplicate definition key returns `conflict`; a duplicate rule name returns `duplicate_rule`; a duplicate action key returns `action_key_exists`. After an uncertain network outcome, list definitions by key/name before retrying. Updates retain omitted definition fields.

**Example: create a class, then a declared relation.** Replace the IDs, URL and token with your own. These commands change definitions in that base; they use synthetic names here. IDs in `domains` and `ranges` must name classes in the same base. To create a rule, call `create_rule` with `subject_type_id`, a `conclusion` (`typing`, `attribute`, `computed` or `relation`), its matching conclusion fields and nonempty `conditions`; `tools/list` gives the current schema. `run_rules` is the separate execution step.

```bash
curl -sS -X POST "https://your-utopia/api/v1/kbs/$KB/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_entity_type","arguments":{"key":"performance_cycle","label":"Performance Cycle"}}}'

curl -sS -X POST "https://your-utopia/api/v1/kbs/$KB/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"create_relation_type","arguments":{"key":"belongs_to_cycle","label":"belongs to cycle","kind":"relation","domains":["<existing-subject-class-uuid>"],"ranges":["<created-class-uuid>"]}}}'
```

Entity **instances** and relationship **assertions** are separate from these definitions. First create a **Statements** source in the Library using the [ingest guide](ingest.md#statements-source--the-structured-push-interface), then call `list_statement_sources` to find its ID. This setup is necessary; the MCP tools do not create a source or issue a source key. `submit_statements` accepts that source ID and a `payload` with `external_id`, `e`, `s`, and `n` in the [statements contract](../../docs/decisions/0054-a-source-may-push-statements-in-the-open-contract.md). The personal token supplies the editor's authorization; the source ingest key is not sent to MCP. The existing source pipeline stores the payload as its own evidence, creates open entity and statement records, and lets typed alignment and review govern what enters the typed graph. Reuse an `external_id` to submit a revised observation; an unchanged retry returns `unchanged`. `submit_statements` does not accept `deleted: true`.

The response distinguishes `action` (`created`, `updated`, or `unchanged`) from the observation's stable `document_id`, `version`, `document_status`, `graph_status`, and open/typed fact counts linked to its current chunk. After a submit, poll `statement_status` by `source_id` and `external_id`, then use `find_entities` and `entity_facts` to verify the actual graph IDs and facts. A zero typed fact count is not a success signal for typed alignment; these counts do not claim a review decision. Bad payloads and a source from another base fail with `isError: true` and a structured error. No direct typed fact write is introduced.

### Declared local actions

An action definition is separate from a business rule and from each action run. `create_action` requires a unique `key`, `label` and one known `kind`:

- `run_rules`: no `config`, no execution arguments; invokes the same materialization as the direct `run_rules` tool.
- `submit_statements`: `config` must contain exactly `source_id` for an existing extraction-enabled statements source in this base. Execution `args` are the source's open-contract `{external_id,e,s,n}` payload. It ingests an observation, not a typed assertion.

`update_action` accepts `id`, the observed `expected_revision`, and any of `label`, `description`, `config`, `enabled`; key and kind stay fixed. Each edit increments `revision`. A stale edit returns `action_revision_conflict`. `list_actions` returns the definitions. The action is only a declaration until `execute_action` runs it. No rule fires an action automatically in this version.

For each execution, pass the `expected_revision` returned by `create_action` or `list_actions`, generate a fresh UUID `request_id`, and reuse both if the response is lost. A new request with a stale revision returns `action_revision_conflict`. The return includes a durable `run` with `id`, `status` (`running`, `succeeded` or `failed`), result or structured error, and a `replayed` flag. The same actor, action, request ID, revision and arguments returns the prior run without calling the handler again, even if the definition was later changed. Reusing an ID with different arguments or revision gives `request_id_conflict`. Poll `action_status` using the run ID. If a run remains `running` after a server interruption, its effect may be uncertain; inspect source and graph state before deliberately starting a new operation. Action handlers never execute arbitrary code, shell commands or user-supplied URLs. The proposed external HTTP action design and automatic rule-to-action bindings are not implemented.

```bash
curl -sS -X POST "https://your-utopia/api/v1/kbs/$KB/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_action","arguments":{"key":"refresh_rules","label":"Refresh rules","kind":"run_rules"}}}'

# Replace ACTION_ID and ACTION_REVISION with the returned result.id and result.revision;
# generate REQUEST_ID once per intended run.
curl -sS -X POST "https://your-utopia/api/v1/kbs/$KB/mcp" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"execute_action\",\"arguments\":{\"id\":\"$ACTION_ID\",\"request_id\":\"$REQUEST_ID\",\"expected_revision\":$ACTION_REVISION,\"args\":{}}}}"
```

For a synthetic class → attribute/relation definition → rule definition → rule execution call sequence, run `bash scripts/examples/mcp_ontology_demo.sh` from the repository root after setting `UTOPIA_MCP_URL` and `UTOPIA_TOKEN`. Set `UTOPIA_STATEMENTS_SOURCE_ID` to an existing statements source ID to also submit two synthetic statements and read their initial processing status. Use a disposable base: the script creates definitions and, with that optional source, submits an observation. Background extraction and alignment may finish later.

The two time axes matter here. `at` reads **world time** (when something was true); `as_of` reads **record time** (what Utopia held at that moment, before it revised it), and `changes` lists what moved on that axis in a window. They are separate parameters on purpose: folded into one they would answer "what happened in March" with "what we learned in March", and both look plausible.

## The external read contract

**RDF export is the supported machine-readable read contract.**
`GET /api/v1/kbs/{kb_id}/export?format=turtle|jsonld` streams the whole base,
including retracted and corrected assertions, both time axes, quoted evidence,
source documents, and derivations linked to their rules and premise statements.
It uses RDF reification, PROV-O and schema.org. This mapping is the compatibility
boundary: a breaking change needs a decision record explaining why.

Entities, assertions, derivations, rules and documents have UUID-based IRIs:
`urn:utopia:kb:{kb_id}:entity:{id}`, `…:fact:{id}`, `…:derived:{id}`,
`…:rule:{id}` and `…:document:{id}`. The same stored object keeps its identity across exports;
rebuilding a graph is not an identity-preserving operation. `?base=https://example.org/`
instead mints `https://example.org/kb/{kb_id}/{kind}/{id}`. Keep the same base when
joining exports. Imported classes and relations retain their original IRIs.

Ontology export includes explicitly stored `owl:inverseOf` and
`rdfs:subPropertyOf` links, using the target property's imported IRI or existing
key-based IRI. It copies declarations within the base; it does not add reciprocal
links or compute a transitive closure.

A derivation links to `…:rule:{id}` through `prov:wasGeneratedBy`. The rule
resource is typed `prov:Activity` and, by family, `urn:utopia:ns:AxiomRule` or
`urn:utopia:ns:BusinessRule`; it carries an `rdfs:label` (the business rule's
name, or the axiom rule's kind). An axiom rule also states `urn:utopia:ns:axiomKind`
(`transitive`, `symmetric`, `inverse` or `sub_property`) and
`urn:utopia:ns:declaredOn`, the predicate the axiom is declared on, which for
`inverse` and `sub_property` is not the conclusion's predicate. A business rule's
criteria, operands and expressions are not exported: a rule IRI identifies the
stored rule, not a historical version of its definition, and a business rule is
edited in place ([#902](https://github.com/deeplethe/utopia/issues/902)).

MCP remains the agent-facing surface. The structured results below use the same
UUIDs, so an integration can join a selected result to the exported ledger.
Ordinary `/api/v1` UI response shapes are **not** a compatibility promise: they
have no OpenAPI contract or deprecation policy. The export route above is the
explicit exception, not a promise covering every route with that prefix.

The export does not yet include conflict/review state ([#564](https://github.com/deeplethe/utopia/issues/564))
or the chunk identity behind a quote. Per-entity export and a SPARQL endpoint are
also not implemented. Neither MCP nor the export promises historical proof
snapshots: `as_of` selects the derivations held then, but proofs use the current
premise links. Source-document links are the stored provenance, not a separately
versioned snapshot of the evidence set.

## Structured results alongside the text

`find_entities`, `search_chunks`, `entity_facts`, `changes`, and the ontology,
rule, statement, and action tools listed above return a JSON object in
`result.structuredContent` beside human-readable `content`. No extra argument
or protocol upgrade is needed. Read the JSON field directly; the text is
presentation, not a format to parse.

Every structured result includes `kb_id`. IDs are opaque UUID strings, timestamps
are RFC3339 with fractional seconds preserved, unknown optional values are `null`,
and empty collections are `[]`. Clients should tolerate additional fields.
Removing or renaming a documented `structuredContent` field is a breaking change
and requires a decision record explaining why.
Failures have `isError: true` and no structured success payload; an empty result
is a successful read, not an error.

| Tool | Structured fields |
|---|---|
| `find_entities` | `entities[]`, in the same ranked order as the text: `id`, `name`, `type_key`, `type_label`, `disambiguator`, `fact_count`. This is a candidate list, not an exhaustive entity export |
| `search_chunks` | `as_of`, `limit`, `limit_reached`, `chunks[]`. Each chunk has `chunk_id`, `document_id`, zero-based `seq`, `filename`, `text`, `truncated`. The text uses the same 800-character cutoff as the prose |
| `entity_facts` | `entity` (`id`, `name`, `type_key`, `type_label`), effective `at`/`as_of`/`before`, `total_facts`, `matched_facts`, `limit`, `truncated`, `facts[]`, `derived_facts[]` |
| `changes` | Effective `since` (inclusive) and `until` (exclusive), `limit`, `limit_reached`, `changes[]`. Events carry `fact_id`, `at`, `kind`, `subject_id`, `subject_name`, `predicate_label`, `object_name`, `object_value`, `confidence`, `valid_from`, `valid_to`, both validity precisions, `document_id`, `filename`, `quote` |

An asserted `facts[]` entry contains `id`, `direction`, `predicate_key`,
`predicate_label`, `inferred`, `temporal`, `other_id`, `other_name`, `object_value`,
`confidence`, `valid_from`, `valid_to`, `valid_from_precision`, `valid_to_precision`,
`holds_from`, `holds_to`, `recorded_at`, `invalidated_at`, `supersedes`, and
deduplicated `document_ids`. `direction=out` means the requested entity is the
subject; `in` means it is the object. `object_value` retains the JSON value and
unit rather than formatting them into a string. `inferred` describes an
unaccepted predicate name; it does **not** mean the fact is derived.
`qualifiers[]` preserves attributes attached to the relation itself: each has
`qualifier_type_id`, `key`, `label`, raw `value`, `entity_id`, and `entity_name`.

A `derived_facts[]` entry contains `id`, `subject_id`, `subject`, `predicate_id`,
`predicate`, `object_id`, `object`, raw `object_value`, `rule`, `rule_name`,
`rule_id`, `attribute_rule_id`, `confidence`, `valid_from`, `valid_to`, both
validity precisions, `derived_at`, and `invalidated_at`. Rule IDs distinguish
axiom rules from business rules. The derivation ID joins to `…:derived:{id}` in
RDF to follow its proof; this result does not expand the proof tree.

`valid_*` describes stated world time; `holds_*` is the interpreted interval used
for filtering assertions. `recorded_at` (or `derived_at`) and `invalidated_at`
describe the stored record-time lifetime. A later invalidation can be present
even when reading a row held at an earlier `as_of`. `before=T` takes precedence
over `as_of` and reports the effective cutoff `T − 1µs`. A `changes[].at` can be
passed to `before` without losing precision. Events have no separate event UUID;
`fact_id` identifies the affected assertion, and can appear in multiple events.

The structure follows the current text selection: assertion filters and the
default 80-fact limit (maximum 300) apply to `facts[]`; derived conclusions only
use `at`/`as_of`/`before` and are listed separately without that limit. `truncated`
reports omitted matching assertions. Search returns at most six chunks and
changes at most 40 events; `limit_reached` means the cap was reached, not that
another result is known to exist. Narrow the query/window to inspect more.
Historical full-text search retains the recall limitation described above.

## What an agent records waits for a nod

`remember` stores the sentence immediately — searchable at once, attributed to the token's owner. The facts extracted from it do **not** enter the graph. They queue in Review as proposals, each shown beneath the sentence it came from, and a person confirms or rejects them one at a time.

That gate is why the tool can be served at all. An agent reads documents in the base, and a document can say "remember that X" — so an agent may be talked into recording something by the very material it was asked to read. A proposal that waits for a person costs a click; an assertion that does not costs a wrong edge on the graph, indistinguishable from one someone meant.

The Review card names the agent, not only the person: several clients can share one owner's identity, and "which one said this" is most of what a reviewer has to go on. Never claim to a user that a fact was added — say the sentence was recorded and its facts await confirmation.

## Try it from a shell

List the tools:

```bash
curl -s -X POST https://your-utopia/api/v1/kbs/$KB/mcp \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

Search, then read the whole document the best hit came from:

```bash
curl -s -X POST https://your-utopia/api/v1/kbs/$KB/mcp \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call",
       "params":{"name":"search_chunks","arguments":{"query":"Series C target"}}}'

curl -s -X POST https://your-utopia/api/v1/kbs/$KB/mcp \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call",
       "params":{"name":"get_document","arguments":{"document_id":"<id from the hit>"}}}'
```

A document id from another base, or a made-up one, comes back as "No document with that id in this knowledge base" — the tool cannot tell you what it is not allowed to see.

## Point a client at it

Most MCP clients accept a remote server as a URL plus headers. For a client that reads a JSON config:

```json
{
  "mcpServers": {
    "utopia-general": {
      "url": "https://your-utopia/api/v1/kbs/<kb_id>/mcp",
      "headers": { "Authorization": "Bearer utp_pat_…" }
    }
  }
}
```

One entry per knowledge base you want the agent to reach. The agent sees the same base the token's owner sees, and nothing else.

## What is not here yet

- **SQL.** `query_data` (a read-only query against a mounted database) is a chat tool and is not served over MCP. It reaches production databases outside this deployment, and what a borrowed agent should be able to ask them is a question this version does not answer.
- **Streaming.** Responses are single JSON-RPC replies; there is no server-sent event channel.
