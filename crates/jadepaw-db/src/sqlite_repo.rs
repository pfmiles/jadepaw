//! SQLite-backed implementation of `SessionRepository`.
//!
//! Uses WAL mode for non-blocking concurrent reads (D-09). Connection pool
//! of 5 connections. All write transactions use `BEGIN IMMEDIATE`.
//!
//! # Design (D-09)
//!
//! - WAL mode enables concurrent readers during writes.
//! - `busy_timeout = 5s` prevents immediate "database is locked" errors.
//! - `foreign_keys = ON` for referential integrity (future table additions).
//! - Migrations run at construction time via `sqlx::migrate!()`.
//!
//! # UUID BLOB Pattern (D-05)
//!
//! `session_id` and `tenant_id` are stored as BLOB (16 bytes) using
//! `Uuid::as_bytes()` for binding and `Uuid::from_slice()` for extraction.

use anyhow::{Context, Result};
use async_trait::async_trait;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;

use jadepaw_core::{SessionId, TenantId};
use uuid::Uuid;

use crate::models::{SessionSnapshot, SessionStatus, SessionSummary};
use crate::repository::SessionRepository;

/// SQLite-backed implementation of `SessionRepository`.
///
/// Owns a `SqlitePool` directly (not behind `Arc`) -- callers wrap the
/// entire repository in `Arc` when sharing across sessions, following
/// the same pattern as `ToolRegistry` owning `DashMap` directly.
pub struct SqliteSessionRepo {
    pool: SqlitePool,
}

impl SqliteSessionRepo {
    /// Create a new repository backed by a SQLite database at `db_path`.
    ///
    /// Enables WAL mode, sets `busy_timeout` to 5s, enables foreign keys,
    /// and creates the database file if it does not exist. Pool size is
    /// capped at 5 connections (SQLite is single-writer; more connections
    /// don't increase write concurrency).
    pub async fn new(db_path: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(db_path)
            .context("invalid database path")?
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .foreign_keys(true)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(opts)
            .await
            .context("failed to create SQLite connection pool")?;

        // Run embedded migrations at construction time.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("failed to run database migrations")?;

        Ok(Self { pool })
    }
}

#[async_trait]
impl SessionRepository for SqliteSessionRepo {
    /// Persist a full session snapshot with upsert semantics.
    ///
    /// Uses INSERT ... ON CONFLICT DO UPDATE so the same method works for
    /// both initial creation (status = idle -> running) and turn-boundary
    /// updates (status stays running, fields are refreshed).
    async fn save(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (session_id, tenant_id, status, messages_json, trace_json,
             guard_config_json, elapsed_ms, iteration_count, created_at, updated_at, termination_reason_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(session_id) DO UPDATE SET
               status = excluded.status,
               messages_json = excluded.messages_json,
               trace_json = excluded.trace_json,
               guard_config_json = excluded.guard_config_json,
               elapsed_ms = excluded.elapsed_ms,
               iteration_count = excluded.iteration_count,
               updated_at = excluded.updated_at,
               termination_reason_json = excluded.termination_reason_json",
        )
        .bind(session_id.as_bytes().as_slice())
        .bind(tenant_id.as_bytes().as_slice())
        .bind(snapshot.status.to_string())
        .bind(&snapshot.messages_json)
        .bind(&snapshot.trace_json)
        .bind(&snapshot.guard_config_json)
        .bind(snapshot.elapsed_ms as i64)
        .bind(snapshot.iteration_count as i32)
        .bind(snapshot.created_at.to_rfc3339())
        .bind(snapshot.updated_at.to_rfc3339())
        .bind(&snapshot.termination_reason_json)
        .execute(&self.pool)
        .await
        .context("failed to save session")?;
        Ok(())
    }

    /// Load a session snapshot by ID.
    ///
    /// Filters by both `session_id` and `tenant_id` for isolation (D-08).
    /// Returns `None` if either doesn't match. Deserializes BLOB columns
    /// back into UUID newtypes and TEXT columns into DateTime.
    async fn load(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
    ) -> Result<Option<SessionSnapshot>> {
        let row = sqlx::query(
            "SELECT session_id, tenant_id, status, messages_json, trace_json,
             guard_config_json, elapsed_ms, iteration_count, created_at, updated_at,
             termination_reason_json
             FROM sessions
             WHERE session_id = ? AND tenant_id = ?",
        )
        .bind(session_id.as_bytes().as_slice())
        .bind(tenant_id.as_bytes().as_slice())
        .fetch_optional(&self.pool)
        .await
        .context("failed to load session")?;

        let Some(row) = row else {
            return Ok(None);
        };

        use sqlx::Row;

        let session_id_blob: Vec<u8> = row.get("session_id");
        let s_id = SessionId::from(Uuid::from_slice(&session_id_blob).context("invalid session_id BLOB")?);

        let tenant_id_blob: Vec<u8> = row.get("tenant_id");
        let t_id =
            TenantId::from(Uuid::from_slice(&tenant_id_blob).context("invalid tenant_id BLOB")?);

        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "idle" => SessionStatus::Idle,
            "running" => SessionStatus::Running,
            "paused" => SessionStatus::Paused,
            "ended" => SessionStatus::Ended,
            other => anyhow::bail!("unknown session status: {}", other),
        };

        let messages_json: String = row.get("messages_json");
        let trace_json: String = row.get("trace_json");
        let guard_config_json: String = row.get("guard_config_json");
        let elapsed_ms: i64 = row.get("elapsed_ms");
        let iteration_count: i32 = row.get("iteration_count");

        let created_at_str: String = row.get("created_at");
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .context("invalid created_at timestamp")?
            .with_timezone(&chrono::Utc);

        let updated_at_str: String = row.get("updated_at");
        let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
            .context("invalid updated_at timestamp")?
            .with_timezone(&chrono::Utc);

        let termination_reason_json: Option<String> = row.get("termination_reason_json");

        Ok(Some(SessionSnapshot {
            session_id: s_id,
            tenant_id: t_id,
            status,
            messages_json,
            trace_json,
            guard_config_json,
            elapsed_ms: elapsed_ms as u64,
            iteration_count: iteration_count as u32,
            created_at,
            updated_at,
            termination_reason_json,
        }))
    }

    /// List all sessions for a tenant (summary-only, no blob columns).
    ///
    /// Stub implementation — full implementation in Plan 05-02 Task 2.
    async fn list_by_tenant(&self, _tenant_id: TenantId) -> Result<Vec<SessionSummary>> {
        anyhow::bail!("not yet implemented: list_by_tenant");
    }

    /// Delete a session by ID and tenant.
    ///
    /// Stub implementation — full implementation in Plan 05-02 Task 2.
    async fn delete(&self, _session_id: SessionId, _tenant_id: TenantId) -> Result<()> {
        anyhow::bail!("not yet implemented: delete");
    }

    /// Update the status of a session.
    ///
    /// Stub implementation — full implementation in Plan 05-02 Task 2.
    async fn update_status(
        &self,
        _session_id: SessionId,
        _tenant_id: TenantId,
        _status: SessionStatus,
    ) -> Result<()> {
        anyhow::bail!("not yet implemented: update_status");
    }

    /// Mark all running sessions as paused (crash recovery).
    ///
    /// Stub implementation — full implementation in Plan 05-02 Task 2.
    async fn mark_running_as_paused(&self) -> Result<Vec<SessionId>> {
        anyhow::bail!("not yet implemented: mark_running_as_paused");
    }
}