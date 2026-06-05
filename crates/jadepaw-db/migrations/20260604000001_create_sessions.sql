-- Create sessions table for Phase 5 session persistence.
-- Supports session pause/resume, crash recovery, and tenant isolation.

CREATE TABLE IF NOT EXISTS sessions (
    session_id      BLOB PRIMARY KEY NOT NULL,
    tenant_id       BLOB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'idle'
                    CHECK (status IN ('idle', 'running', 'paused', 'ended')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    messages_json   TEXT NOT NULL DEFAULT '[]',
    trace_json      TEXT NOT NULL DEFAULT '[]',
    guard_config_json TEXT NOT NULL DEFAULT '{}',
    elapsed_ms      INTEGER NOT NULL DEFAULT 0,
    iteration_count INTEGER NOT NULL DEFAULT 0,
    termination_reason_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_tenant_created
    ON sessions (tenant_id, created_at);