# Phase 05: Session Memory - Pattern Map

**Mapped:** 2026-06-04
**Files analyzed:** 13 (7 new, 5 modified, 1 migration)
**Analogs found:** 12 / 13

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/jadepaw-db/Cargo.toml` | config | -- | `crates/jadepaw-bus/Cargo.toml` | exact |
| `crates/jadepaw-db/src/lib.rs` | module-root | -- | `crates/jadepaw-bus/src/lib.rs` | exact |
| `crates/jadepaw-db/src/repository.rs` | trait | CRUD | `crates/jadepaw-core/src/host_functions.rs` | role-match |
| `crates/jadepaw-db/src/sqlite_repo.rs` | service | CRUD | `crates/jadepaw-agent/src/tool_registry.rs` | partial |
| `crates/jadepaw-db/src/models.rs` | model | CRUD | `crates/jadepaw-core/src/types.rs` | role-match |
| `crates/jadepaw-db/src/migrations.rs` | utility | transform | `crates/jadepaw-wasm/src/limits/` (ResourceLimiter chain pattern) | partial |
| `crates/jadepaw-db/migrations/20260604000001_create_sessions.sql` | migration | -- | RESEARCH.md (sqlx::migrate! pattern) | no-codebase |
| `crates/jadepaw-agent/src/window.rs` | service | transform | `crates/jadepaw-agent/src/llm.rs` | role-match |
| `crates/jadepaw-agent/src/loop.rs` (MODIFIED) | controller | event-driven | (existing file -- self-referencing) | self |
| `crates/jadepaw-agent/src/guard.rs` (MODIFIED) | model | -- | (existing file -- self-referencing) | self |
| `crates/jadepaw-agent/src/lib.rs` (MODIFIED) | module-root | request-response | (existing file -- self-referencing) | self |
| `crates/jadepaw-core/src/agent_types.rs` (MODIFIED) | model | -- | (existing file -- self-referencing) | self |
| `crates/jadepaw-agent/tests/context_window.rs` (NEW) | test | -- | `crates/jadepaw-agent/tests/agent_loop.rs` | role-match |

## Pattern Assignments

### `crates/jadepaw-db/Cargo.toml` (config)

**Analog:** `crates/jadepaw-bus/Cargo.toml`

**Full template** (entire file):
```toml
[package]
name = "jadepaw-db"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
jadepaw-core = { path = "../jadepaw-core" }
sqlx = { workspace = true, features = ["sqlite", "chrono", "uuid", "runtime-tokio-rustls"] }
serde_json = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
anyhow = "1.0"
tracing = "0.1"
async-trait = "0.1"

[features]
default = ["single-node"]
single-node = []
cluster = ["sqlx-postgres"]

[dependencies.sqlx-postgres]
# Aliased for conditional compilation; sqlx with postgres feature when cluster is enabled
```

**Key conventions:**
- `version.workspace = true`, `edition.workspace = true`, `license.workspace = true` -- all crates use workspace inheritance
- `path = "../jadepaw-core"` for internal deps -- relative path pattern from `jadepaw-bus/Cargo.toml` lines 8-9
- Feature flags `single-node` and `cluster` mirror `jadepaw-bus` feature toggle pattern (lines 13-15)

---

### `crates/jadepaw-db/src/lib.rs` (module root)

**Analog:** `crates/jadepaw-bus/src/lib.rs`

**Full template** (entire file):
```rust
//! # jadepaw-db
//!
//! Database persistence layer for session state. Exposes a `SessionRepository`
//! trait with SQLite backing (single-node) and migration support.
//!
//! ## What lives here
//!
//! - `SessionRepository` trait -- canonical persistence contract
//! - `SqliteSessionRepo` -- single-node SQLite implementation
//! - `SessionSnapshot`, `SessionSummary`, `SessionStatus` -- data models
//! - Embedded SQLx migrations for schema management
//!
//! ## What does NOT live here
//!
//! - Agent loop or ReAct orchestration (see jadepaw-agent)
//! - Wasm runtime or instance management (see jadepaw-wasm)
//! - HTTP/WS transport (see jadepaw-gateway)
//! - Core data types (see jadepaw-core)

pub mod migrations;
pub mod models;
pub mod repository;
pub mod sqlite_repo;

pub use models::{SessionSnapshot, SessionStatus, SessionSummary};
pub use repository::SessionRepository;
pub use sqlite_repo::SqliteSessionRepo;
```

**Key conventions** from `jadepaw-bus/src/lib.rs` lines 1-17:
- Doc-comment crate header with `## What lives here` / `## What does NOT live here` sections
- Module declarations before re-exports
- `pub use` for the public API surface at the bottom

---

### `crates/jadepaw-db/src/repository.rs` (trait, CRUD)

**Analog:** `crates/jadepaw-core/src/host_functions.rs` -- trait-in-core, impl-downstream pattern + `crates/jadepaw-core/src/tool.rs` (async_trait + Send + Sync bounds)

**Imports pattern** (from `host_functions.rs` lines 24-26, `tool.rs` lines 22-23):
```rust
use async_trait::async_trait;
use anyhow::Result;

use jadepaw_core::{SessionId, TenantId};
use crate::models::{SessionSnapshot, SessionStatus, SessionSummary};
```

**Trait definition pattern** (from `host_functions.rs` lines 43-47, `tool.rs` lines 217-252):
```rust
/// Repository trait for session persistence.
///
/// All methods require both `session_id` and `tenant_id` as mandatory
/// parameters — the type system enforces isolation at every call site (D-08).
///
/// # Additive-only policy
///
/// Methods may be added, never removed. CI must verify all implementors.
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// Persist a full session snapshot.
    ///
    /// Uses upsert semantics: inserts on first save, updates on subsequent
    /// saves for the same session.
    async fn save(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
        snapshot: SessionSnapshot,
    ) -> Result<()>;

    /// Load a session snapshot by ID.
    ///
    /// Returns `None` if the session does not exist or the tenant_id
    /// does not match (isolation-preserving lookup).
    async fn load(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
    ) -> Result<Option<SessionSnapshot>>;

    /// List all sessions for a tenant (summary-only, no blob columns).
    async fn list_by_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<Vec<SessionSummary>>;

    /// Delete a session and all its data.
    async fn delete(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
    ) -> Result<()>;

    /// Update the status of a session.
    ///
    /// Enforces the state machine: idle->running->paused->running->ended.
    /// Returns an error if the transition is invalid.
    async fn update_status(
        &self,
        session_id: SessionId,
        tenant_id: TenantId,
        status: SessionStatus,
    ) -> Result<()>;

    /// Scan for sessions with `status = 'running'` and mark them `paused`.
    ///
    /// Used for crash recovery on startup (D-07).
    async fn mark_running_as_paused(&self) -> Result<Vec<SessionId>>;
}
```

**Key conventions:**
- `#[async_trait]` + `Send + Sync` bounds from `tool.rs` line 218 (Tool trait pattern)
- `anyhow::Result` return type (not `JadepawError`) -- repository is an infrastructure concern separate from domain error types
- Doc comments on every trait method -- follows the `HostFunctions` pattern where every method has a doc comment explaining contract + security (lines 44-84)
- `session_id` + `tenant_id` as mandatory first two params on every method (D-08 isolation enforcement)

---

### `crates/jadepaw-db/src/models.rs` (model, CRUD)

**Analog:** `crates/jadepaw-core/src/types.rs` (newtype wrappers) + `crates/jadepaw-core/src/agent_types.rs` (Serialize/Deserialize derives)

**Imports pattern** (from `types.rs` lines 5-8, `agent_types.rs` lines 16-17):
```rust
use jadepaw_core::{SessionId, TenantId, AgentTerminationReason};
use serde::{Deserialize, Serialize};
use std::fmt;
```

**Struct definitions pattern** (from `agent_types.rs` lines 24-33, `error.rs` lines 13-43):
```rust
/// A full session snapshot for persistence.
///
/// Contains both normalized metadata and JSON blob columns.
/// All JSON fields are pre-serialized strings -- the caller is responsible
/// for calling `serde_json::to_string()` before constructing this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub status: SessionStatus,
    pub messages_json: String,
    pub trace_json: String,
    pub guard_config_json: String,
    pub elapsed_ms: u64,
    pub iteration_count: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub termination_reason_json: Option<String>,
}

/// A lightweight session summary (no blob columns).
///
/// Used for `list_by_tenant()` to avoid loading large JSON blobs
/// when only metadata is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub termination_reason: Option<AgentTerminationReason>,
    pub message_count: usize,
    pub turn_count: usize,
    pub elapsed_ms: u64,
}
```

**Enum pattern** for `SessionStatus` (from `agent_types.rs` lines 107-141 -- `AgentTerminationReason` enum):
```rust
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
```

**Key conventions:**
- `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]` on all structs -- matches `AgentRequest`/`AgentResponse` pattern (agent_types.rs lines 24, 48)
- `#[serde(rename = "...")]` on enum variants -- matches `AgentTerminationReason` display pattern
- `impl fmt::Display` for enums -- follows the standard Rust display pattern used in `AgentTerminationReason` (agent_types.rs lines 143-176)

---

### `crates/jadepaw-db/src/sqlite_repo.rs` (service, CRUD)

**Analog:** `crates/jadepaw-agent/src/tool_registry.rs` -- DashMap-based concurrent service with `impl Default`

**Imports pattern** (from `tool_registry.rs` lines 22-26):
```rust
use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;
use tracing;

use jadepaw_core::{SessionId, TenantId};

use crate::models::{SessionSnapshot, SessionStatus, SessionSummary};
use crate::repository::SessionRepository;
```

**Struct + constructor pattern** (from `tool_registry.rs` lines 34-48, GuardConfig lines 23-29):
```rust
/// SQLite-backed implementation of `SessionRepository`.
///
/// Uses WAL mode for non-blocking concurrent reads (D-09). Connection pool
/// of 3-5 connections. All write transactions use `BEGIN IMMEDIATE`.
pub struct SqliteSessionRepo {
    pool: SqlitePool,
}

impl SqliteSessionRepo {
    /// Create a new repository backed by a SQLite database at `db_path`.
    ///
    /// Enables WAL mode, sets `busy_timeout` to 5s, and enables
    /// foreign keys via `after_connect` pragmas.
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

        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("failed to run database migrations")?;

        Ok(Self { pool })
    }
}
```

**Impl pattern** (from `tool_registry.rs` lines 107-175 -- `call_tool()` method structure):
```rust
#[async_trait]
impl SessionRepository for SqliteSessionRepo {
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
               termination_reason_json = excluded.termination_reason_json"
        )
        .bind(session_id.as_bytes())
        .bind(tenant_id.as_bytes())
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

    // load(), list_by_tenant(), delete(), update_status(), mark_running_as_paused()
    // follow the same pattern: sqlx::query + .bind() + .context() + Ok(())
}
```

**Key conventions:**
- `SqlitePool` stored as a struct field (not `Arc<SqlitePool>`) -- follows the `ToolRegistry` pattern where `DashMap` is owned directly (not behind Arc) since the struct itself is wrapped in Arc by the caller
- `anyhow::Context` for error context -- every `.await?` is preceded by `.context("description")` matching the loop.rs pattern (line 171)
- `sqlx::query` (runtime) for blob-heavy queries -- D-05 specifies runtime queries for JSON columns
- `uuid::Uuid::as_bytes()` for BLOB binding -- D-05 specifies BLOB PRIMARY KEY for session_id/tenant_id

---

### `crates/jadepaw-db/src/migrations.rs` (utility, transform)

**Analog:** `crates/jadepaw-wasm/src/engine.rs` -- initialization module that wraps crate initialization logic.

**Pattern:**
```rust
//! Database migration management.
//!
//! Uses sqlx::migrate!() to embed migrations at compile time.
//! Migrations are run in `SqliteSessionRepo::new()`.

// Migrations are embedded via sqlx::migrate!("../migrations") in sqlite_repo.rs.
// This module exists as documentation of the migration strategy and as a
// placeholder for future direct migration access (e.g., standalone CLI commands).
```

**No direct code analog exists** -- migrations are a mostly-declarative concern. The `engine.rs` module provides the structural pattern: a module that exists primarily to organize initialization logic, with the actual implementation living in a sibling module that calls into it.

---

### `crates/jadepaw-db/migrations/20260604000001_create_sessions.sql` (migration)

**No codebase analog.** New pattern for this project. Placed here for completeness.

**Pattern from RESEARCH.md (lines 434-440):**
```sql
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
```

---

### `crates/jadepaw-agent/src/window.rs` (service, transform)

**Analog:** `crates/jadepaw-agent/src/llm.rs` -- per-module service file with public functions + private helpers + `#[cfg(test)]` tests

**Imports pattern** (from `llm.rs` lines 17-28):
```rust
use anyhow::Context;
use async_openai::{
    Client,
    config::Config,
    types::chat::ChatCompletionRequestMessage,
};
use tiktoken_rs::{cl100k_base_singleton, o200k_base_singleton};
use crate::guard::GuardConfig;
```

**Core structure pattern** (from `llm.rs` -- public fn + private helpers + tests at bottom):
```rust
//! Context window management.
//!
//! Handles token counting, context window compression, and message
//! summarization for the ReAct loop. See ROADMAP.md MEM-01.
//!
//! # Design (D-02, D-02a, D-02b)
//!
//! - Token counting runs synchronously before each LLM call (~10ms CPU)
//! - Summarization spawns asynchronously outside the hot ReAct path
//! - Hybrid approach: summarize old turns, keep recent N=5 verbatim

/// Count tokens in a message history using a model-appropriate BPE tokenizer.
///
/// Uses tiktoken-rs singletons for sub-ms counting performance.
/// Model selection determines which encoding singleton to use.
pub fn count_tokens(messages: &[ChatCompletionRequestMessage], model: &str) -> usize {
    let bpe = model_tokenizer(model);
    let mut total = 0;
    for msg in messages {
        // Serialize each message to its chat template form for accurate counting.
        // The chat template adds role markers (~3 tokens per message) that
        // raw content counting misses.
        let text = format_message_for_counting(msg);
        total += bpe.encode_with_special_tokens(&text).len();
    }
    total
}

/// Check whether context window compression should be triggered.
///
/// Returns true if total tokens exceed 65% of the model's context window.
/// The check is O(num_messages) -- ~10ms for typical session lengths (D-02b).
pub fn should_compress(messages: &[ChatCompletionRequestMessage], model: &str) -> bool {
    let total = count_tokens(messages, model);
    let context_window = model_context_window(model);
    total as f64 > context_window as f64 * 0.65
}

// ── private helpers ──────────────────────────────────────────────

fn model_tokenizer(model: &str) -> tiktoken_rs::CoreBPE {
    match model {
        "gpt-4o" | "gpt-4.1" | "gpt-5" | "o1" | "o3" => o200k_base_singleton(),
        "gpt-4" | "gpt-3.5-turbo" => cl100k_base_singleton(),
        _ => o200k_base_singleton(), // Fallback: covers most current models
    }
}

fn model_context_window(model: &str) -> usize {
    // Conservative estimates; caller can override
    match model {
        "gpt-4o" | "gpt-4.1" => 128_000,
        "gpt-4" => 8_192,
        "gpt-4-32k" => 32_768,
        "gpt-3.5-turbo" => 4_096,
        "gpt-3.5-turbo-16k" => 16_384,
        _ => 128_000, // Default to GPT-4o window
    }
}

fn format_message_for_counting(msg: &ChatCompletionRequestMessage) -> String {
    // Serialize the message to a string representation that matches
    // the chat template format the LLM actually sees.
    // On the happy path, `serde_json::to_string(&msg)` is sufficient
    // because async-openai's types derive Serialize.
    serde_json::to_string(msg).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_count_triggers_at_65_percent() {
        // MEM-01: verify threshold fires
    }

    #[test]
    fn summarization_preserves_recent_n() {
        // MEM-01: recent N=5 turns verbatim
    }

    #[test]
    fn compression_respects_token_budget() {
        // MEM-01: post-compression under limit
    }
}
```

**Key conventions** from `llm.rs`:
- Module-level doc comment explaining design decisions (lines 1-16)
- Public functions documented with `///` doc comments (every public fn in llm.rs has docs)
- Private helpers separated by a visual section marker (`// ── private helpers`)
- `#[cfg(test)] mod tests` at the bottom of the file (lines 347-469 in llm.rs)
- Functions return concrete types, not `Result` (token counting is infallible) -- consistent with `llm::build_initial_messages()` returning `Vec<ChatCompletionRequestMessage>` directly (line 86-90)

---

### `crates/jadepaw-agent/src/loop.rs` (MODIFIED, controller, event-driven)

This is a modification to an existing file. The pattern additions are:

**Integration point 1: Before LLM call (token check + windowing) -- insert after line 151 (after `for turn in 0..`):**
```rust
// [NEW] Token count check + context window compression (MEM-01, D-02b)
// Runs synchronously before each LLM call; ~10ms CPU, negligible vs LLM latency.
let should_window = window::should_compress(&messages, model);
if should_window {
    // [NEW] Summarize older turns, keep recent N=5 verbatim (D-01).
    // The summary replaces turns older than N. This runs synchronously
    // on the first invocation (fast, no LLM call needed for summarizing
    // into a structured prefix). The async LLM-based summarization is
    // spawned separately when needed.
    messages = window::compress_context(messages, model, guard_config.recent_turns());
    tracing::info!(
        turn = turn,
        session_id = %session_id,
        "context window compressed"
    );
}
```

**Integration point 2: After observation (persist checkpoint) -- insert after line 265 (after `messages.push(observation_msg)`):**
```rust
// [NEW] Persist turn-boundary checkpoint (MEM-02, D-06)
// Full-state snapshot: messages, trace, guard config, iteration count.
// This runs after each completed think-act-observe cycle.
if let Some(repo) = session_repo {
    let elapsed = elapsed_accumulator + start.elapsed().as_millis() as u64;
    let snapshot = SessionSnapshot {
        session_id,
        tenant_id,
        status: SessionStatus::Running,
        messages_json: serde_json::to_string(&messages)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to serialize messages for checkpoint");
                "[]".to_string()
            }),
        trace_json: serde_json::to_string(&trace)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to serialize trace for checkpoint");
                "[]".to_string()
            }),
        guard_config_json: serde_json::to_string(guard_config)
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to serialize guard config for checkpoint");
                "{}".to_string()
            }),
        elapsed_ms: elapsed.0,
        iteration_count: turn + 1,
        created_at: session_created_at,
        updated_at: chrono::Utc::now(),
        termination_reason_json: None,
    };
    repo.save(session_id, tenant_id, snapshot).await?;
}
```

**Signature change** for `react_loop()` -- add parameters:
```rust
pub async fn react_loop(
    guard_config: &GuardConfig,
    session: &mut SessionHandle,
    llm_client: &Client<Box<dyn Config>>,
    model: &str,
    system_prompt: &str,
    user_message: &str,
    context: Option<&str>,
    tx: &mpsc::Sender<ReActStep>,
    tool_registry: &ToolRegistry,
    // [NEW] Session persistence
    session_repo: Option<&dyn SessionRepository>,
    session_id: SessionId,
    tenant_id: TenantId,
    // [NEW] Resume state
    pre_existing_messages: Vec<ChatCompletionRequestMessage>,
    pre_existing_trace: Vec<ReActStep>,
    elapsed_accumulator: Duration,
    start_turn: u32,
    session_created_at: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Vec<ReActStep>> {
```

**Key conventions:**
- New code inserted at the EXACT integration points documented by the TODO(WR-04) at loop.rs lines 144-149
- `Option<&dyn SessionRepository>` makes persistence optional (backward compatible with tests that don't need DB)
- Checkpoint failures are logged but not fatal -- an error in persistence should not crash the agent loop

---

### `crates/jadepaw-agent/src/guard.rs` (MODIFIED, model)

**New field on `GuardConfig`** (lines 23-29, extends existing struct):
```rust
#[derive(Clone)]
pub struct GuardConfig {
    /// Maximum number of ReAct loop iterations.
    pub max_iterations: u32,
    /// Maximum wall-clock time allowed for the entire agent run.
    pub wall_clock_timeout: Duration,

    // [NEW] Context window configuration (MEM-01)
    /// Number of recent turns to preserve verbatim during context compression.
    /// Default: 5.
    pub recent_turns: u32,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            wall_clock_timeout: Duration::from_secs(300),
            recent_turns: 5, // [NEW]
        }
    }
}

impl GuardConfig {
    // [NEW] convenience accessor
    pub fn recent_turns(&self) -> u32 {
        self.recent_turns
    }
}
```

---

### `crates/jadepaw-agent/src/lib.rs` (MODIFIED, module-root)

**New module declaration** (after existing `pub mod tool_registry;` at line 27):
```rust
pub mod window;
```

**New re-export** (after existing re-exports at lines 162-168):
```rust
pub use window::{compress_context, count_tokens, should_compress};
```

**New `run_agent()` signature -- add `resume_from` support.** The existing `run_agent()` at lines 64-158 stays but a new `resume_session()` function is added:

```rust
/// Resume a paused session from a database snapshot.
///
/// Loads the snapshot, reconstructs the ReAct loop state, acquires a fresh
/// Wasm Store from InstancePool, and continues execution from the next turn.
/// The Wasm Store is NOT restored from disk (D-06a) -- only conversational
/// state is restored.
pub async fn resume_session(
    session_id: SessionId,
    tenant_id: TenantId,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    repo: &dyn SessionRepository,
    tool_registry: Option<Arc<ToolRegistry>>,
) -> core::result::Result<(AgentResponse, impl Stream<Item = core::result::Result<Event, Infallible>>), JadepawError>
{
    // 1. Load snapshot from DB
    let snapshot = repo.load(session_id, tenant_id)
        .await
        .map_err(|e| JadepawError::agent_terminated(
            AgentTerminationReason::InfrastructureError {
                reason: format!("failed to load session: {}", e),
                turn: 0,
            },
        ))?
        .ok_or_else(|| JadepawError::agent_terminated(
            AgentTerminationReason::InfrastructureError {
                reason: format!("session not found: {}", session_id),
                turn: 0,
            },
        ))?;

    // 2. Deserialize messages and trace from JSON blobs
    let messages: Vec<ChatCompletionRequestMessage> = serde_json::from_str(&snapshot.messages_json)
        .map_err(|e| /* ... */)?;
    let pre_trace: Vec<ReActStep> = serde_json::from_str(&snapshot.trace_json)
        .map_err(|e| /* ... */)?;

    // 3. Create fresh Wasm Store (D-06a: Store NOT serialized)
    let state = SessionState::with_defaults(temp_dir());
    let mut handle = pool.acquire(session_id, state).await.map_err(|e| /* ... */)?;

    // 4. Create SSE channel
    let (tx, sse_stream) = stream::create_sse_channel();

    // 5. Reconstruct GuardConfig from snapshot
    let guard_config = /* deserialize from guard_config_json */;

    // 6. Run react_loop() with pre-existing state
    let trace = guard::run_with_guard(&guard_config, || {
        r#loop::react_loop(
            /* ... all params ... */
            Some(repo),
            session_id,
            tenant_id,
            messages,           // pre-existing
            pre_trace,           // pre-existing
            Duration::from_millis(snapshot.elapsed_ms),
            snapshot.iteration_count,
            snapshot.created_at,
        )
    }).await;

    // 7. Extract final answer, update status to ended, return response
    // ...
}
```

**Key conventions:**
- `run_agent()` stays unchanged (backward compatible) -- `resume_session()` is new (additive-only per CONTEXT.md line 105)
- Same error mapping pattern as `run_agent()` (JadepawError::agent_terminated with InfrastructureError variant)
- Same pattern of creating SSE channel + running under guard + dropping tx before checking trace

---

### `crates/jadepaw-core/src/agent_types.rs` (MODIFIED, model)

**New field on `AgentRequest`** (after `context: Option<String>` at line 31):
```rust
/// Optional session ID to resume from.
///
/// When set, the agent loads the session snapshot from the database
/// and continues execution from where it left off.
#[serde(default)]
pub resume_from: Option<SessionId>,
```

---

### `crates/jadepaw-agent/tests/context_window.rs` (NEW, test)

**Analog:** `crates/jadepaw-agent/tests/agent_loop.rs` -- integration test file pattern

**Full template** (from `agent_loop.rs` lines 1-12):
```rust
//! Tests for context window management: token counting, threshold
//! detection, and message compression behavior.
//!
//! These tests verify MEM-01 requirements: auto-compression triggers
//! at 65% context window and preserves recent N turns verbatim.

use jadepaw_agent::window;

// ── Test: token counting detects overflow ─────────────────────────

#[test]
fn token_count_triggers_at_65_percent() {
    // Build messages that exceed 65% of a known context window
    // Verify should_compress() returns true
}

#[test]
fn token_count_does_not_trigger_below_65_percent() {
    // Verify should_compress() returns false for small histories
}

#[test]
fn summarization_preserves_recent_n() {
    // Build messages, compress, verify last N=5 turns are verbatim
}

#[test]
fn compression_respects_token_budget() {
    // Post-compression: verify total tokens < context window
}
```

**Key conventions** from `agent_loop.rs`:
- Module-level doc comment with `//!` explaining test scope (lines 1-4)
- Use of `// ── section divider` comments for visual organization
- `#[test]` for unit-like tests, `#[tokio::test(flavor = "multi_thread")]` for async tests
- Test helper functions defined at the top of the file
- Integration tests live in `tests/` directory (not inline `#[cfg(test)]`)

---

## Shared Patterns

### Trait-in-Core, Impl-Downstream

**Source:** `crates/jadepaw-core/src/host_functions.rs` (trait) + `crates/jadepaw-wasm/src/session.rs` (impl)
**Apply to:** `SessionRepository` trait (in jadepaw-core or jadepaw-db) + `SqliteSessionRepo` impl

```
crates/jadepaw-core/src/host_functions.rs    # trait definition
crates/jadepaw-wasm/src/session.rs           # impl (via HostFunctions for Wasm)
```

Pattern: Define trait in core (zero internal deps), implement in domain crate. For Phase 5, the trait can live in `jadepaw-db/src/repository.rs` (CONTEXT.md D-04 specifies new crate), but the pattern of `#[async_trait] + Send + Sync + Result<>` return types is identical.

### Additive-Only Interface Design

**Source:** `HostFunctions` trait changelog comments (host_functions.rs lines 38-41) and `Tool` trait methods (tool.rs lines 218-252)
**Apply to:** All new public APIs

Every public API addition follows the rule: add new functions, never remove or change existing signatures. The AgentRequest gains an `Option<SessionId>` field with `#[serde(default)]` (never a required field). The `run_agent()` function stays unchanged; `resume_session()` is a separate new function.

### Crate Structure

**Source:** `crates/jadepaw-bus/` (full crate)
**Apply to:** `crates/jadepaw-db/`

| Pattern Element | Source | Target |
|----------------|--------|--------|
| `Cargo.toml` with workspace inheritance | `jadepaw-bus/Cargo.toml` | `jadepaw-db/Cargo.toml` |
| `src/lib.rs` with doc header + module decls + re-exports | `jadepaw-bus/src/lib.rs` | `jadepaw-db/src/lib.rs` |
| Feature gates (`single-node`, `cluster`) | `jadepaw-bus/Cargo.toml` lines 13-15 | `jadepaw-db/Cargo.toml` |

### Error Handling

**Source:** `crates/jadepaw-agent/src/loop.rs` lines 157-177 (`.context()` + structured error mapping)
**Apply to:** `sqlite_repo.rs` (DB errors), `window.rs` (serialization errors), `lib.rs` (resume errors)

Pattern: `.await.context("human description")?` on every fallible operation. Repository methods return `anyhow::Result`. The agent layer (lib.rs) maps infrastructure errors to `JadepawError::AgentTerminated` with `InfrastructureError` variant (consistent with existing LLM/pool error mapping at lib.rs lines 83-90).

### Test Structure

**Source:** `crates/jadepaw-agent/tests/agent_loop.rs` (integration test)
**Apply to:** `jadepaw-agent/tests/context_window.rs`, `jadepaw-agent/tests/session_persistence.rs`, `jadepaw-db/tests/`

```
tests/
  context_window.rs      # MEM-01 tests
  session_persistence.rs  # MEM-02 tests
  agent_loop.rs           # existing (unchanged)
  sse_streaming.rs        # existing (unchanged)
  termination.rs          # existing (unchanged)
```

Test pattern: `#[tokio::test(flavor = "multi_thread")]` for async tests, helper functions at top of file, visual section dividers.

### Module Organization

**Source:** `crates/jadepaw-agent/src/lib.rs` lines 23-28 (module declarations)
**Apply to:** `jadepaw-agent/src/lib.rs` (add `pub mod window;`)

New modules are added as `pub mod window;` in alphabetical position within the existing module list. Re-exports follow at the bottom of the file.

### Serde Derives

**Source:** `agent_types.rs` lines 16, 24, 48, 62 (all types derive Serialize/Deserialize)
**Apply to:** `models.rs` (SessionSnapshot, SessionSummary, SessionStatus)

All data types that cross crate boundaries MUST derive `Serialize, Deserialize`. This is already true for `ReActStep`, `AgentResponse`, `AgentTerminationReason`, `SessionId`, `TenantId` -- the new types follow the same convention.

### UUID BLOB Pattern

**Source:** `types.rs` lines 13-39 (SessionId is `Uuid` newtype with `Deref<Target=Uuid>`)
**Apply to:** `sqlite_repo.rs` (binding as `.as_bytes()`, extracting with `Uuid::from_bytes()`)

```rust
// Binding (sqlite_repo.rs)
.bind(session_id.as_bytes())  // session_id derefs to Uuid, Uuid::as_bytes() -> &[u8; 16]

// Extraction (sqlite_repo.rs load method)
let raw: Vec<u8> = row.get("session_id");
let session_id = SessionId(Uuid::from_slice(&raw)?);
```

---

## No Analog Found

Files with no close match in the codebase:

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/jadepaw-db/migrations/20260604000001_create_sessions.sql` | migration | -- | First SQL migration in the project. No prior `.sql` migration files exist. The sqlx::migrate!() macro pattern is well-documented in RESEARCH.md lines 434-440. Use the exact SQL from that section. |

---

## Metadata

**Analog search scope:** `crates/jadepaw-core/src/`, `crates/jadepaw-agent/src/`, `crates/jadepaw-agent/tests/`, `crates/jadepaw-bus/`, `crates/jadepaw-wasm/src/`
**Files scanned:** 13 source files read (loop.rs, guard.rs, lib.rs, llm.rs, stream.rs, agent_types.rs, error.rs, types.rs, host_functions.rs, tool.rs, tool_registry.rs, capabilities.rs, session.rs) + 2 crate structures (jadepaw-bus/Cargo.toml + lib.rs)
**Pattern extraction date:** 2026-06-04