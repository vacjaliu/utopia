# 0068 · A local action declares a known handler

- **Status:** Implemented · 2026-10-05 · migration 0106; open: external HTTP actions and automatic rule triggers
- **Related:** [0034](0034-an-action-is-a-declared-call.md), [0050](0050-an-action-attempt-keeps-its-identity-and-uncertain-outcome.md), [0054](0054-a-source-may-push-statements-in-the-open-contract.md), [0067](0067-mcp-edits-ontology-definitions-through-the-workbench.md)

## Context

An agent can define vocabulary and rules over MCP, but a rule definition is distinct from an action definition. The general outbound HTTP action registry in 0034 and its uncertain-outcome protocol in 0050 are proposed designs. Utopia has no approved external action sender. An agent still needs a real, inspectable action path for work the server can already do inside a knowledge base.

## Decision

Introduce KB-local `ontology_actions`, each choosing one built-in handler: `run_rules` (the existing materialization route) or `submit_statements` (the existing source-attributed statements route, with one configured source ID). The definition stores no code, URL, credential, template or arbitrary executable JSON. The handler and configuration are validated when created or edited. An editor with a write-scoped personal token can create, update and execute one over MCP; viewers can list definitions and read run status. Disable is supported; deletion is deliberately omitted to retain the definition and run ledger. This is a manual agent call, not rule-triggered execution.

Every edit and execute request carries the observed `expected_revision`; a stale new operation fails rather than silently using a changed handler configuration. An execute request also carries a caller-generated UUID `request_id`. The server commits one `running` attempt with actor, action, definition revision and arguments before invoking the handler. A replay of the same actor/action/request ID, revision and arguments returns that row without executing again, even after the definition is edited or disabled. Reusing an ID with different arguments or revision fails. A new ID intentionally creates a new operation. If the process dies after the attempt is recorded, `running` is an uncertain outcome: neither a replay nor a worker automatically retries it. The caller can inspect the run and the affected graph/source before choosing a new operation. A completed attempt stores success data or a structured failure; audit records its action and request IDs. Existing MCP authorization is checked on every call.

`submit_statements` keeps the open observation and alignment path. Action success means ingestion succeeded; it does not certify typed binding or human review. `run_rules` reports the existing engine's counts. Neither handler sends a network request to a user-supplied destination, invokes shell code, or mutates production systems outside Utopia. The proposed deployment-wide HTTP actions and rule-to-action bindings remain separate work requiring their own settled security and business semantics.

## Tradeoffs

The attempt ledger preserves at-most-one handler invocation by this MCP API for a request ID. A crash can leave an unresolved `running` row, and a database failure after an internal effect can do the same. The record is intentionally truthful rather than claiming exact completion. The existing source's `external_id` also protects observation identity, and rule materialization is internally idempotent; neither property removes the need to inspect an uncertain attempt. This first version does not offer preview, schedules, automatic recovery, external HTTP handlers or rule-triggered actions.
