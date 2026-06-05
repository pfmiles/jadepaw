//! Session repository trait -- canonical persistence contract.
//!
//! The `SessionRepository` trait defines the data access interface for session
//! state. All methods require both `session_id` and `tenant_id` as mandatory
//! parameters -- the type system enforces isolation at every call site (D-08).
//!
//! # Additive-only policy
//!
//! Methods may be added, never removed. CI must verify all implementors
//! cover every method.
//!
//! # Design (D-08)
//!
//! - Every method takes `session_id` + `tenant_id` as the first two params.
//! - `load()` returns `None` if either ID doesn't match -- no cross-tenant leaks.
//! - `list_by_tenant()` filters by tenant, not session -- no unrestricted listing.
//! - `update_status()` enforces the state machine transitions (D-07).

use async_trait::async_trait;
use anyhow::Result;

use jadepaw_core::{SessionId, TenantId};

use crate::models::{SessionSnapshot, SessionStatus, SessionSummary};

/// Repository trait for session persistence.
///
/// All methods require both `session_id` and `tenant_id` as mandatory
/// parameters -- the type system enforces isolation at every call site (D-08).
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Persist a full session snapshot.
    ///
    /// Uses upsert semantics: inserts on first save, updates on subsequent
    /// saves for the same session. Callers are responsible for ensuring
    /// the snapshot's JSON fields (`messages_json`, `trace_json`,
    /// `guard_config_json`) are valid JSON strings.
    async fn save(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<()>;

    /// Load a session snapshot by ID.
    ///
    /// Returns `None` if the session does not exist or the `tenant_id`
    /// does not match (isolation-preserving lookup). The returned snapshot
    /// includes all JSON blob columns -- callers should use `list_by_tenant()`
    /// for lightweight metadata queries.
    async fn load(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
    ) -> Result<Option<SessionSnapshot>>;

    /// List all sessions for a tenant (summary-only, no blob columns).
    ///
    /// Returns lightweight `SessionSummary` records excluding the large
    /// JSON blob columns (`messages_json`, `trace_json`, `guard_config_json`).
    /// Sessions are ordered by `created_at` descending (most recent first).
    async fn list_by_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<SessionSummary>>;

    /// Delete a session and all its data.
    ///
    /// Both `session_id` and `tenant_id` must match for the deletion to
    /// succeed. Returns `Ok(())` even if the session does not exist
    /// (idempotent delete).
    async fn delete(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
    ) -> Result<()>;

    /// Update the status of a session.
    ///
    /// Enforces the state machine: idle -> running -> paused -> running -> ended.
    /// Returns an error if the transition is invalid (e.g., ended -> running).
    /// Both `session_id` and `tenant_id` must match for the update to succeed.
    async fn update_status(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
        status: SessionStatus,
    ) -> Result<()>;

    /// Scan for sessions with `status = 'running'` and mark them `paused`.
    ///
    /// Used for crash recovery on startup (D-07). Returns the list of
    /// session IDs that were transitioned from running to paused.
    async fn mark_running_as_paused(&self) -> Result<Vec<SessionId>>;
}