//! Session data models: `SessionStatus`, `SessionSnapshot`, and `SessionSummary`.
//!
//! These types form the persistence layer's data contract. All types derive
//! `Serialize`/`Deserialize` for JSON blob serialization and wire transport.
//!
//! # Design (D-03, D-07)
//!
//! - `SessionStatus` is a four-variant state machine: idle -> running -> paused -> ended.
//! - `SessionSnapshot` contains full session state including JSON blob columns
//!   for message history, trace, and configuration.
//! - `SessionSummary` is a lightweight subset (no blob columns) used for listing.

use jadepaw_core::{AgentTerminationReason, SessionId, TenantId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Session lifecycle state machine (D-07).
///
/// Transitions: idle -> running -> paused -> running -> ended.
/// Enforced at the DB layer via CHECK constraint and at the Rust layer
/// via `SqliteSessionRepo::update_status()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session created but not yet running.
    #[serde(rename = "idle")]
    Idle,
    /// Session is actively executing.
    #[serde(rename = "running")]
    Running,
    /// Session has been paused (explicit API or crash recovery).
    #[serde(rename = "paused")]
    Paused,
    /// Session has ended (normal completion or termination).
    #[serde(rename = "ended")]
    Ended,
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Ended => write!(f, "ended"),
        }
    }
}

/// A full session snapshot for persistence.
///
/// Contains both normalized metadata and JSON blob columns.
/// All JSON fields are pre-serialized strings -- the caller is responsible
/// for calling `serde_json::to_string()` before constructing this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// Owning tenant identifier.
    pub tenant_id: TenantId,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// JSON-serialized message history (`Vec<ChatCompletionRequestMessage>`).
    pub messages_json: String,
    /// JSON-serialized execution trace (`Vec<ReActStep>`).
    pub trace_json: String,
    /// JSON-serialized guard configuration (`GuardConfig`).
    pub guard_config_json: String,
    /// Accumulated wall-clock time in milliseconds.
    pub elapsed_ms: u64,
    /// Number of ReAct loop iterations completed.
    pub iteration_count: u32,
    /// Session creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// JSON-serialized termination reason (`AgentTerminationReason`), if session ended.
    pub termination_reason_json: Option<String>,
}

/// A lightweight session summary (no blob columns).
///
/// Used for `list_by_tenant()` to avoid loading large JSON blobs
/// when only metadata is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// Owning tenant identifier.
    pub tenant_id: TenantId,
    /// Current lifecycle status.
    pub status: SessionStatus,
    /// Session creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// How the session ended, if applicable.
    pub termination_reason: Option<AgentTerminationReason>,
    /// Approximate count of messages in the conversation.
    pub message_count: usize,
    /// Number of ReAct turns completed.
    pub turn_count: usize,
    /// Accumulated wall-clock time in milliseconds.
    pub elapsed_ms: u64,
}