-- MCP-authored, KB-local actions invoke only registered internal handlers.
-- No user-authored code, URL, credential or external dispatch is stored here.
CREATE TABLE ontology_actions (
    id UUID PRIMARY KEY,
    kb_id UUID NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    label TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    kind TEXT NOT NULL CHECK (kind IN ('run_rules', 'submit_statements')),
    config JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(config) = 'object'),
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (kb_id, key),
    UNIQUE (kb_id, id)
);

CREATE TABLE ontology_action_runs (
    id UUID PRIMARY KEY,
    kb_id UUID NOT NULL,
    action_id UUID NOT NULL,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    request_id UUID NOT NULL,
    action_revision INTEGER NOT NULL,
    args JSONB NOT NULL CHECK (jsonb_typeof(args) = 'object'),
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    result JSONB,
    error JSONB,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    FOREIGN KEY (kb_id, action_id) REFERENCES ontology_actions(kb_id, id) ON DELETE CASCADE,
    UNIQUE (action_id, actor_id, request_id)
);
CREATE INDEX ontology_action_runs_kb_started_idx ON ontology_action_runs(kb_id, started_at DESC);
