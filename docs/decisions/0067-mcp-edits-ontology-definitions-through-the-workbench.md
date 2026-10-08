# 0067 · MCP edits ontology definitions through the workbench

- **Status:** Implemented · 2026-10-05 · MCP ontology editor; open: direct typed assertion remains outside this decision
- **Related:** [0014](0014-identity-from-the-person-scope-from-the-token.md), [0021](0021-a-rule-reads-attributes-and-concludes-a-type.md), [0044](0044-the-ontology-is-a-view-over-what-documents-say.md), [0054](0054-a-source-may-push-statements-in-the-open-contract.md)

## Context

The HTTP workbench can create and edit class and property definitions and executable business rules. The MCP surface exposed their read side but not their edit side. An external agent with an editor's personal token could not build a vocabulary through its normal interface. The CLI has no corresponding user-scoped ontology interface.

The ontology's instance path is different: statements have sources and evidence and are first recorded in the open graph. Typed binding is governed by alignment. A new direct `entities` or `facts` insert tool would bypass that contract. The separately implemented KB-local action registry is described in [0068](0068-a-local-action-declares-a-known-handler.md).

## Decision

Expose MCP-only tools for listing and editing class and relation definitions, creating and updating business rules, reading their versions, and explicitly running the existing materialization engine. Invoke the existing HTTP handler functions from these tools. This retains their role checks, store validation, audit records, reindex jobs and execution behavior. MCP's existing token scope and per-call role check remain the outer gate. Editing and execution require both a write-scoped token and editor role. The tool list reflects the effective permission.

For entity and relationship instances, reuse the existing statements-source contract. The agent selects an existing source in the same base, submits `{external_id,e,s,n}` through MCP, and the exact source ingestion path persists a versioned document and queues extraction. The MCP personal token replaces the source ingest key at this boundary; the key is never exposed to the agent. Return the ingest action and document ID separately from processing and typed fact counts, and provide a status lookup by source and external ID. A response saying `created` means the observation was ingested, not that a typed fact was asserted. A repeated external ID follows the source's existing update/unchanged semantics.

Keep the new tools out of the in-app chat tool list. They are intended for an external agent acting with an explicit write token, while in-app chat has its own tool policy. MCP responses use structured success data and stable error codes. Unknown arguments fail instead of being silently ignored. Referenced classes and qualifiers must exist in the same base before a create proceeds. Partial definition updates read the existing definition and retain omitted fields. A duplicate key or rule name fails with a stable code; clients can query existing definitions after an uncertain network result before retrying. The store's two definition create paths commit their definition and parent/signature links together, avoiding an orphan definition if those links fail.

Do not add direct typed instance writes or arbitrary action JSON. The user chose the governed statements path for this first version. A statements source must already exist; creating a new source through MCP would generate or manage another persistent ingest credential and is outside this cut. Defining an agent-facing, source-attributed typed assertion route requires a separate decision about identity, provenance, review, update and temporal semantics. The local action execution design is recorded separately in [0068](0068-a-local-action-declares-a-known-handler.md); it does not implement the proposed outbound HTTP registry.

## Alternatives

- A CLI wrapper around the HTTP API would add a second authentication and argument surface without using Utopia's already deployed MCP endpoint.
- Calling store writes directly from MCP would duplicate workbench orchestration, particularly audit and alignment reindexing.
- Reinterpreting `remember` as a typed write would misreport a proposal as a graph assertion.

## Consequences

An editor can establish a vocabulary and validated business rules from an MCP client, inspect the IDs and definitions returned, and run the real rule engine. The same client can submit attributable observations that create open entities and statements, then follow their processing and typed alignment. It cannot directly assert typed instances. [0068](0068-a-local-action-declares-a-known-handler.md) describes the separately added executable local actions. Those limits are explicit in the MCP guide.
