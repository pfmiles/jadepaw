//! SQLite-backed implementation of `SkillRepository`.
//!
//! Shares the same `SqlitePool` as `SqliteSessionRepo` — the pool handles
//! WAL mode, busy_timeout, and foreign_keys configuration. This repository
//! does NOT create its own pool or run migrations; those are the caller's
//! responsibility.
//!
//! # Design (D-09, D-10)
//!
//! - Receives an already-configured `SqlitePool` via constructor.
//! - All queries use dual-key (skill_id, tenant_id) isolation.
//! - UUIDs stored as BLOB (16 bytes) following the same pattern as sessions.
//! - Timestamps stored as RFC3339 strings, parsed with chrono.
//!
//! # UUID BLOB Pattern
//!
//! `skill_id` and `tenant_id` are stored as BLOB (16 bytes) using
//! `Uuid::as_bytes()` for binding and `Uuid::from_slice()` for extraction.

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use jadepaw_core::{SkillId, TenantId};
use uuid::Uuid;

use crate::skill_models::{SkillIndexRecord, SkillIndexSummary};
use crate::skill_repository::SkillRepository;

/// SQLite-backed implementation of `SkillRepository`.
///
/// Holds a reference to the shared `SqlitePool`. Does NOT create its own
/// pool or run migrations — the caller is responsible for pool creation
/// and `sqlx::migrate!()`.
pub struct SqliteSkillRepo {
    pool: SqlitePool,
}

impl SqliteSkillRepo {
    /// Create a new skill repository using the given connection pool.
    ///
    /// The pool must already be configured with WAL mode, busy_timeout,
    /// and foreign keys enabled. The caller must run migrations separately
    /// via `sqlx::migrate!("../jadepaw-db/migrations").run(&pool)` before
    /// using this repository.
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SkillRepository for SqliteSkillRepo {
    /// Bulk-upsert skill index entries.
    ///
    /// Uses INSERT OR REPLACE for each entry. The ON CONFLICT clause with
    /// WHERE tenant_id ensures that a (skill_id, tenant_id) pair uniquely
    /// identifies a record — a different tenant_id for the same skill_id
    /// is treated as a separate record.
    async fn sync_index(&self, entries: &[SkillIndexRecord]) -> Result<()> {
        for entry in entries {
            sqlx::query(
                "INSERT OR REPLACE INTO skill_index
                 (skill_id, tenant_id, name, description, version, tools_json, file_path, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(entry.skill_id.as_bytes().as_slice())
            .bind(entry.tenant_id.as_bytes().as_slice())
            .bind(&entry.name)
            .bind(&entry.description)
            .bind(&entry.version)
            .bind(&entry.tools_json)
            .bind(&entry.file_path)
            .bind(entry.created_at.to_rfc3339())
            .bind(entry.updated_at.to_rfc3339())
            .execute(&self.pool)
            .await
            .context("failed to sync skill index entry")?;
        }
        Ok(())
    }

    /// List all skills for a tenant (summary-only).
    ///
    /// Returns lightweight `SkillIndexSummary` records excluding the
    /// `tools_json` and `file_path` columns. Ordered by `created_at`
    /// descending.
    async fn list_by_tenant(&self, tenant_id: TenantId) -> Result<Vec<SkillIndexSummary>> {
        let rows = sqlx::query(
            "SELECT skill_id, tenant_id, name, description, version
             FROM skill_index
             WHERE tenant_id = ?
             ORDER BY created_at DESC",
        )
        .bind(tenant_id.as_bytes().as_slice())
        .fetch_all(&self.pool)
        .await
        .context("failed to list skills by tenant")?;

        let mut summaries = Vec::with_capacity(rows.len());
        for row in &rows {
            let skill_id_blob: Vec<u8> = row.get("skill_id");
            let s_id = SkillId::from(
                Uuid::from_slice(&skill_id_blob).context("invalid skill_id BLOB")?,
            );

            let tenant_id_blob: Vec<u8> = row.get("tenant_id");
            let t_id = TenantId::from(
                Uuid::from_slice(&tenant_id_blob).context("invalid tenant_id BLOB")?,
            );

            let name: String = row.get("name");
            let description: String = row.get("description");
            let version: Option<String> = row.get("version");

            summaries.push(SkillIndexSummary {
                skill_id: s_id,
                tenant_id: t_id,
                name,
                description,
                version,
            });
        }

        Ok(summaries)
    }

    /// Look up a single skill by tenant and name.
    ///
    /// Both `tenant_id` and `name` must match — dual-key isolation prevents
    /// cross-tenant information disclosure. Returns the full record including
    /// `tools_json` and `file_path`.
    async fn get_by_name(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Result<Option<SkillIndexRecord>> {
        let row = sqlx::query(
            "SELECT skill_id, tenant_id, name, description, version, tools_json, file_path, created_at, updated_at
             FROM skill_index
             WHERE tenant_id = ? AND name = ?
             LIMIT 1",
        )
        .bind(tenant_id.as_bytes().as_slice())
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .context("failed to get skill by name")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let skill_id_blob: Vec<u8> = row.get("skill_id");
        let s_id =
            SkillId::from(Uuid::from_slice(&skill_id_blob).context("invalid skill_id BLOB")?);

        let tenant_id_blob: Vec<u8> = row.get("tenant_id");
        let t_id =
            TenantId::from(Uuid::from_slice(&tenant_id_blob).context("invalid tenant_id BLOB")?);

        let r_name: String = row.get("name");
        let description: String = row.get("description");
        let version: Option<String> = row.get("version");
        let tools_json: String = row.get("tools_json");
        let file_path: String = row.get("file_path");

        let created_at_str: String = row.get("created_at");
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .context("invalid created_at timestamp")?
            .with_timezone(&chrono::Utc);

        let updated_at_str: String = row.get("updated_at");
        let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
            .context("invalid updated_at timestamp")?
            .with_timezone(&chrono::Utc);

        Ok(Some(SkillIndexRecord {
            skill_id: s_id,
            tenant_id: t_id,
            name: r_name,
            description,
            version,
            tools_json,
            file_path,
            created_at,
            updated_at,
        }))
    }

    /// Delete a skill from the index.
    ///
    /// Both `skill_id` and `tenant_id` must match for the deletion to
    /// succeed. Idempotent — returns `Ok(())` even if no rows matched.
    async fn delete(&self, skill_id: SkillId, tenant_id: TenantId) -> Result<()> {
        sqlx::query("DELETE FROM skill_index WHERE skill_id = ? AND tenant_id = ?")
            .bind(skill_id.as_bytes().as_slice())
            .bind(tenant_id.as_bytes().as_slice())
            .execute(&self.pool)
            .await
            .context("failed to delete skill index entry")?;
        Ok(())
    }
}