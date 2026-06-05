-- Create skill_index table for Phase 6 skill metadata caching.
-- SQLite caches parsed skill metadata for fast listing while the filesystem
-- remains the source of truth (D-09). Dual-key isolation (skill_id, tenant_id)
-- prevents cross-tenant data leakage.

CREATE TABLE IF NOT EXISTS skill_index (
    skill_id    BLOB PRIMARY KEY NOT NULL,
    tenant_id   BLOB NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version     TEXT,
    tools_json  TEXT NOT NULL DEFAULT '[]',
    file_path   TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_skill_index_tenant_name
    ON skill_index (tenant_id, name);

CREATE INDEX IF NOT EXISTS idx_skill_index_tenant_created
    ON skill_index (tenant_id, created_at);