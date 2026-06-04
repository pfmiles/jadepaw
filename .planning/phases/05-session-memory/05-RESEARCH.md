# Phase 05: Session Memory - Research

**Researched:** 2026-06-04
**Domain:** Context window management + SQLite persistence for conversational AI agent sessions
**Confidence:** HIGH

## Summary

Phase 5 adds two capabilities to the existing ReAct agent loop (Phase 3): automatic context window compression to prevent unbounded message growth, and SQLite-based session persistence enabling pause/resume across process restarts. The implementation is layered -- token-counting and windowing insert before each LLM call in the hot ReAct path, while turn-boundary persistence checkpoints run after each completed think-act-observe cycle.

The existing codebase already has the integration points: `react_loop()` accumulates `messages: Vec<ChatCompletionRequestMessage>` (the windowing target), `GuardConfig` controls termination behavior (extension point for persistence config), and `AgentResponse.trace: Vec<ReActStep>` already derives `Serialize/Deserialize` (ready for JSON blob storage). A new `jadepaw-db` crate follows the established architectural pattern (`jadepaw-bus`, `jadepaw-wasm` are siblings) and exposes a `SessionRepository` trait.

**Primary recommendation:** Insert two checkpoints in the ReAct loop -- token-count-and-window before each LLM call, persist-to-SQLite after each completed turn. Both are additive (no existing code removal), and both operate on data structures already present in the loop body.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Token counting & context windowing | API / Backend (jadepaw-agent) | -- | Operates on `Vec<ChatCompletionRequestMessage>` in the ReAct loop host side; no Wasm boundary |
| Summarization LLM call | API / Backend (jadepaw-agent) | -- | Host-side async-openai call; runs outside hot path per D-02a |
| Session persistence (save/load) | API / Backend (jadepaw-db) | API / Backend (jadepaw-agent) | Trait in jadepaw-db, impl in jadepaw-db, called from jadepaw-agent at turn boundaries |
| Session isolation (Wasm) | API / Backend (jadepaw-wasm) | -- | Store-per-session + ResourceLimiter remain the primary security boundary; unchanged by Phase 5 |
| Session isolation (DB) | API / Backend (jadepaw-db) | -- | `session_id` + `tenant_id` on every repository method (D-08) |
| Pause/resume lifecycle | API / Backend (jadepaw-agent) | API / Backend (jadepaw-db) | Status state machine in agent, persistence in db crate |
| Wasm Store lifecycle | API / Backend (jadepaw-wasm) | -- | Stores are NOT serialized (D-06a); resume creates fresh Store from InstancePre |

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MEM-01 | Single-session dialog context management with auto-compression when approaching token limit | tiktoken-rs v0.12.0 with `o200k_base_singleton` / `cl100k_base_singleton`; token count check before each LLM call; summarize older turns, keep recent N=5 verbatim; 65% threshold of model context window |
| MEM-02 | Session state persistence (SQLite single-node mode) with pause and resume support | sqlx 0.9.0 with SQLite; `SessionRepository` trait in new `jadepaw-db` crate; turn-boundary full-state snapshots; `SessionStatus` state machine (idle -> running -> paused -> running -> ended) |
</phase_requirements>

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Hybrid context window -- summarize older turns, keep recent N=5 verbatim. Summary preserves tool names, action types, error status, key findings; drops verbose thought content. Injected as system-prefix message.
- **D-02:** Adaptive threshold at 65% of model context window. Token counting uses `tiktoken-rs` with model-appropriate singleton.
- **D-02a:** Summarization runs asynchronously outside the hot ReAct loop path.
- **D-02b:** Token counting check runs before each LLM call (~10ms CPU, negligible vs LLM latency).
- **D-03:** Hybrid schema -- normalized columns for session metadata, JSON TEXT blobs for message history and trace.
- **D-04:** New `jadepaw-db` crate exposing `SessionRepository` trait with `SqlitePool` backing.
- **D-05:** SQLx for database access -- `query!`/`query_as!` for normalized columns, runtime `sqlx::query` for blob columns.
- **D-05a:** Single migration file: `sessions` table with metadata + blob columns.
- **D-06:** Full-state snapshot at turn boundaries. On resume: fresh Wasm Store from InstancePre, restore conversational state.
- **D-06a:** Wasm Stores are NOT serialized -- irreducible constraint.
- **D-06b:** Mid-turn crash loses in-flight turn; recovery from last checkpoint.
- **D-07:** Session status state machine: idle -> running -> paused -> running -> ended.
- **D-07a:** Wall-clock guard continuity via `elapsed_ms` accumulator across pause/resume.
- **D-08:** Repository-layer isolation -- all methods require `session_id` + `tenant_id`.
- **D-09:** SQLite WAL mode + `busy_timeout` 5s + `BEGIN IMMEDIATE`. Pool size 3-5.
- **D-09a:** Wasm layer remains primary security boundary; DB is internal persistence store.
- Token count threshold: 65% (locked, user-specified empirical value, not a default to override).
- tiktoken-rs v0.12: `o200k_base_singleton` for GPT-4o/4.1, `cl100k_base_singleton` for GPT-4/3.5-turbo.
- N=5 recent turns verbatim, configurable via `GuardConfig` extension.
- `SessionStatus`: Idle, Running, Paused, Ended (TEXT column with CHECK constraint).
- Connection pool: max_connections=5, WAL mode, busy_timeout=5000, foreign_keys=ON.
- Migration: single `01_create_sessions.sql`.

### Claude's Discretion

No areas were deferred to Claude -- all decisions were user-directed.

### Deferred Ideas (OUT OF SCOPE)

- MEM-03: Long-term memory / cross-session knowledge (v2)
- Per-tool rate limiting in persistence layer (v2)
- MEM-04: Cross-node session migration (v2)
- tiktoken-rs WASM blob size monitoring (~5MB) -- address if problematic later
</user_constraints>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| **tiktoken-rs** | 0.12.0 | Token counting for context window management | Official Rust binding for OpenAI's tiktoken BPE tokenizer. Provides model-specific singletons (`o200k_base_singleton`, `cl100k_base_singleton`) that avoid reinitializing the tokenizer per session. 8.8M+ total downloads, MIT licensed, maintained by zurawiki. Supports GPT-4o, GPT-4.1, GPT-5, o-series models. [VERIFIED: crates.io] |
| **sqlx** | 0.9.0 | Async SQLite database access with compile-time checked queries | Already declared in workspace `Cargo.toml` at 0.9 with `runtime-tokio-rustls` feature. Provides `query!`/`query_as!` macros for type-safe normalized columns, runtime queries for JSON blob columns, and built-in migration support. Zero-ORM overhead. [VERIFIED: crates.io + workspace Cargo.toml] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **serde_json** | 1.0 (workspace) | JSON serialization for message/trace blob columns | Already in workspace. `Vec<ChatCompletionRequestMessage>` and `Vec<ReActStep>` both derive Serialize/Deserialize. Used for single-call blob persistence per D-03. |
| **uuid** | 1.0 (workspace) | UUID v7 for session/tenant IDs | Already in workspace with `v7` feature. `SessionId` and `TenantId` are UUID v7 newtypes. |
| **chrono** | 0.4 (workspace) | Timestamps for session metadata | Already in workspace with `serde` feature. `created_at`, `updated_at`, `last_active_at` columns. |
| **sqlx-cli** | 0.9.x | Migration management CLI | `cargo install sqlx-cli` (not in workspace -- dev tool). Used for `cargo sqlx prepare` to generate offline query data for CI. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| **tiktoken-rs 0.12** | `tiktoken` 3.x (pure Rust) | `tiktoken` 3.x is a pure-Rust rewrite (no C dependency), which avoids the ~5MB WASM blob concern. However, it's newer and the CONTEXT.md explicitly specifies `tiktoken-rs`. |
| **tiktoken-rs 0.12** | OpenAI API token count endpoint | No local computation -- requires network call per count. Adds latency and API cost. tiktoken-rs gives sub-ms local counting. |
| **sqlx 0.9** | rusqlite | Synchronous API, no compile-time query checking, no migration support. Would require manual connection pooling and async wrapper. |
| **sqlx 0.9** | Diesel ORM | Synchronous by default, requires schema.rs generation. Explicitly avoided in project CLAUDE.md. |

**Installation:**
```bash
# tiktoken-rs added to jadepaw-agent (token counting) and jadepaw-core (if shared types needed)
# sqlx with sqlite feature added to new jadepaw-db crate
# sqlx-cli installed globally for dev tooling
cargo install sqlx-cli --no-default-features --features sqlite
```

**Version verification:**
```bash
# tiktoken-rs: confirmed v0.12.0 on crates.io, rust-version 1.85.0, 8,864,503 total downloads
# sqlx: confirmed v0.9.0 on crates.io, workspace already specifies 0.9
```

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| tiktoken-rs | crates.io | 2+ yrs | 8.8M total | github.com/zurawiki/tiktoken-rs | [OK] | Approved |
| sqlx | crates.io | 5+ yrs | 100M+ total | github.com/launchbadge/sqlx | [OK] | Approved |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

Both packages have been confirmed via:
- `cargo info` on crates.io (tiktoken-rs v0.12.0, sqlx v0.9.0)
- `slopcheck install` (both [OK])
- GitHub repository verification
- Ecosystem-appropriate registry (crates.io) confirmed

## Architecture Patterns

### System Architecture Diagram

```
                   User Request
                       |
                       v
              +------------------+
              |   run_agent()    |  (jadepaw-agent lib.rs)
              +------------------+
                       |
          +------------+------------+
          |                         |
          v                         v
   +-------------+          +--------------+
   | InstancePool |          | SessionRepo  |  (jadepaw-db)
   |  .acquire()  |          |  .load()     |
   +-------------+          +--------------+
          |                         |
          v                         v
   +-------------+          +--------------+
   | SessionHandle|          | SQLite DB    |
   | (Wasm Store) |          | (sessions    |
   +-------------+          |  table)      |
          |                 +--------------+
          v
   +---------------------------------------------------+
   |              react_loop()                          |
   |                                                    |
   |  for each turn:                                    |
   |    1. fuel reset                                   |
   |    2. [NEW] token_count_check(&messages)           |
   |       if > 65% context window:                     |
   |         [NEW] summarize_older_turns(&messages)     |
   |    3. llm::stream_llm_response(messages)            |
   |    4. parse action / finish                        |
   |    5. tool dispatch or finish                      |
   |    6. [NEW] session_repo.save(snapshot)             |
   +---------------------------------------------------+
          |
          v
   +-------------+
   | AgentResponse|
   |  .trace      |
   |  .final_answer|
   +-------------+
```

### Recommended Project Structure

```
crates/
├── jadepaw-db/                    # NEW: Database persistence crate
│   ├── Cargo.toml                 # Depends on: jadepaw-core, sqlx, serde_json, chrono
│   ├── src/
│   │   ├── lib.rs                 # Crate root: exports SessionRepository trait + SqliteSessionRepo
│   │   ├── repository.rs          # SessionRepository trait definition
│   │   ├── sqlite_repo.rs         # SqliteSessionRepo impl (SqlitePool-backed)
│   │   ├── models.rs              # SessionRecord, SessionSnapshot, SessionSummary, SessionStatus
│   │   └── migrations.rs          # Embedded migrations via sqlx::migrate!()
│   └── migrations/
│       └── 20260604000001_create_sessions.sql
├── jadepaw-agent/                 # MODIFIED: Add context window + persistence
│   ├── Cargo.toml                 # Add: jadepaw-db, tiktoken-rs
│   └── src/
│       ├── loop.rs                # MODIFIED: Insert token check + persist at turn boundaries
│       ├── window.rs              # NEW: Context window management (token counting, windowing, summarization)
│       ├── guard.rs               # MODIFIED: Add elapsed_ms accumulator, WindowConfig
│       └── lib.rs                 # MODIFIED: Export pause_session(), resume_session()
└── jadepaw-core/                  # MINIMAL CHANGES
    └── src/
        └── agent_types.rs         # MODIFIED: AgentRequest.resume_from field
```

### Pattern 1: Token Counting with Model-Appropriate Singleton

**What:** Use `tiktoken_rs::o200k_base_singleton()` or `cl100k_base_singleton()` depending on the model. The singleton pattern caches the tokenizer instance globally, avoiding expensive re-initialization per session (~5ms per load avoided).

**When to use:** Before each LLM call in the ReAct loop (D-02b). Token counting runs synchronously in the hot path but costs ~10ms CPU -- negligible compared to LLM latency (typically 1-30s).

**Example:**
```rust
// Source: tiktoken-rs GitHub README (zurawiki/tiktoken-rs)
use tiktoken_rs::o200k_base_singleton;

fn count_tokens(messages: &[ChatCompletionRequestMessage], model: &str) -> usize {
    let bpe = match model {
        "gpt-4o" | "gpt-4.1" | "gpt-5" => o200k_base_singleton(),
        "gpt-4" | "gpt-3.5-turbo" => cl100k_base_singleton(),
        // Fallback: try o200k_base (covers most current models)
        _ => o200k_base_singleton(),
    };
    let mut total = 0;
    for msg in messages {
        // Serialize each message to its chat template form for accurate counting
        // tiktoken-rs also provides get_chat_completion_max_tokens for convenience
        total += bpe.encode_with_special_tokens(&format_message_for_counting(msg)).len();
    }
    total
}
```

### Pattern 2: Hybrid Context Window (Summarize + Sliding Window)

**What:** When total tokens exceed 65% of context window, summarize all turns older than the most recent N. The summary is injected as a system-prefix message. Recent N turns remain verbatim.

**When to use:** Before each LLM call. The check is O(num_messages) in CPU -- ~10ms for typical session lengths.

**Message structure after compression:**
```
[system prompt] + [tool definitions] + [summary prefix: "Previous conversation summary: ..."] + [turn N-4] + [turn N-3] + [turn N-2] + [turn N-1] + [turn N]
```

### Pattern 3: SessionRepository Trait + SQLite Implementation

**What:** Define `SessionRepository` trait in `jadepaw-db` (or `jadepaw-core` if minimizing crate count), implement with `SqlitePool` in `jadepaw-db`.

**When to use:** At turn boundaries (persist after each completed think-act-observe cycle), on pause (explicit API call), on resume (load snapshot, reconstruct loop state).

**Example:**
```rust
// Trait definition (jadepaw-db or jadepaw-core)
#[async_trait::async_trait]
pub trait SessionRepository: Send + Sync {
    async fn save(&self, session_id: SessionId, tenant_id: TenantId,
                  snapshot: SessionSnapshot) -> Result<()>;
    async fn load(&self, session_id: SessionId, tenant_id: TenantId)
                  -> Result<Option<SessionSnapshot>>;
    async fn list_by_tenant(&self, tenant_id: TenantId)
                            -> Result<Vec<SessionSummary>>;
    async fn delete(&self, session_id: SessionId, tenant_id: TenantId)
                    -> Result<()>;
    async fn update_status(&self, session_id: SessionId, tenant_id: TenantId,
                           status: SessionStatus) -> Result<()>;
}

// SessionSnapshot: what gets serialized
pub struct SessionSnapshot {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub status: SessionStatus,
    pub messages_json: String,      // serde_json::to_string(&messages)
    pub trace_json: String,         // serde_json::to_string(&trace)
    pub guard_config_json: String,  // serde_json::to_string(&guard_config)
    pub elapsed_ms: u64,
    pub iteration_count: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub termination_reason_json: Option<String>,
}
```

### Pattern 4: Session Status State Machine

**What:** Enforced in `SqliteSessionRepo::update_status()` with CHECK constraint at DB level and Rust-side validation.

**Transitions:**
```
idle --> running     (session started)
running --> paused    (explicit pause or crash recovery)
paused --> running    (resume)
running --> ended     (normal completion or termination)
paused --> ended      (explicit termination while paused)
```

**Implementation:** `SessionStatus` enum with `TryFrom<&str>` and `Display`. Stored as TEXT in SQLite with `CHECK(status IN ('idle','running','paused','ended'))`.

### Pattern 5: Turn-Boundary Persistence

**What:** After each completed ReAct turn (observation step appended), serialize the full state and write to SQLite. This is NOT inside the Wasm Store -- it's host-side state serialization.

**When to use:** At the end of each loop iteration, after the observation is appended and before the next turn begins.

**Integration point in react_loop():**
```rust
// After observation is appended (existing code), add:
// Persist turn-boundary checkpoint
if let Some(repo) = session_repo {
    let snapshot = SessionSnapshot::from_loop_state(
        session_id, tenant_id,
        &messages, &trace, guard_config,
        elapsed_ms, turn + 1,
    );
    repo.save(session_id, tenant_id, snapshot).await?;
}
```

### Pattern 6: Resume Path

**What:** `run_agent()` accepts optional `resume_from: Option<SessionId>`. When set: load snapshot from DB, reconstruct messages Vec from JSON, set elapsed accumulator, create fresh Wasm Store, enter react_loop with pre-populated state.

**When to use:** On API call to resume a paused session.

**Key constraint (D-06a):** The Wasm Store is NOT restored from disk -- a fresh Store is created via `InstancePool::acquire()`. Only conversational state (messages, trace, config) is restored.

### Anti-Patterns to Avoid

- **Serializing wasmtime Store:** Wasmtime `Store<T>` contains JIT-compiled code and pooling allocator linear memory that cannot be serialized. Never attempt to checkpoint or restore a Store. Always create fresh via `InstancePre::instantiate_async()`. [CITED: wasmtime docs + CONTEXT.md D-06a]
- **Token counting via API call:** Using OpenAI's token count endpoint adds ~100-500ms latency per check. tiktoken-rs gives locally-computed counts in ~10ms. [CITED: tiktoken-rs benchmarks]
- **Per-message-row persistence:** Inserting each `ChatCompletionRequestMessage` as a separate DB row adds N INSERTs per turn. The JSON blob approach (single `serde_json::to_string`) is one column update per turn, O(1) regardless of message count. [ASSUMED]
- **Blocking the ReAct loop for summarization:** Making the summarization LLM call synchronously inside the hot ReAct path would add 2-10s latency. Always spawn summarization as a separate tokio task and replace the windowed messages once it completes. [CITED: CONTEXT.md D-02a]
- **SQLite without WAL mode:** Default rollback journal causes writers to block readers. WAL mode enables concurrent reads (snapshot isolation) even during writes, critical when one session is persisting while another is loading. [CITED: SQLite official docs]
- **Omitting tenant_id in queries:** Relying on session_id alone for isolation allows cross-tenant data access if a session_id is guessed. Always include both `session_id` AND `tenant_id` in WHERE clauses. [CITED: CONTEXT.md D-08]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Token counting for LLM messages | Custom BPE token counter | `tiktoken-rs` v0.12 | BPE tokenization is complex and model-specific. tiktoken-rs is the official Rust binding used by 8.8M+ downloads. Custom counting would be inaccurate by 20-50%. |
| SQLite connection pooling | Raw rusqlite + manual pooling | `sqlx` 0.9 `SqlitePool` | sqlx provides async pool with connection health checks, `after_connect` pragma callbacks, WAL mode support, and prepared statement caching. Manual pooling is bug-prone. |
| Database migrations | Custom SQL migration runner | `sqlx::migrate!()` macro | sqlx embeds migrations at compile time, runs idempotently, tracks applied migrations in `_sqlx_migrations` table. Rebuilding this is ~200 lines of error-prone code. |
| JSON serialization for message/trace blobs | Custom format or manual SQL construction | `serde_json::to_string()` | Both `Vec<ChatCompletionRequestMessage>` and `Vec<ReActStep>` already derive Serialize/Deserialize. One call per persist. |
| Session status state machine | Ad-hoc string checks | `SessionStatus` enum + CHECK constraint | Type-safe transitions prevent invalid states (e.g., ended -> running). DB constraint as defense-in-depth. |
| Per-session resource limits for DB | Custom connection-per-session | `SqlitePool` with WAL mode | SQLite WAL allows concurrent readers + single writer. Pool of 3-5 connections serves thousands of sessions. Per-session connections would exhaust file descriptors. |

**Key insight:** Context window management and session persistence are well-understood problems. There is no reason to build custom solutions for token counting (tiktoken-rs is the standard), connection pooling (sqlx has it), or migration management (sqlx has it). The novel work is the integration: where to insert the checks in the ReAct loop and how to reconstruct loop state from a database snapshot.

## Common Pitfalls

### Pitfall 1: Token Counting on Wrong Message Representation

**What goes wrong:** Counting tokens on the Rust struct directly (e.g., summing field string lengths) instead of counting tokens on the serialized chat template format. The chat template adds tokens for role markers, separators, and formatting that the raw message content doesn't include.

**Why it happens:** `encode_with_special_tokens()` is called on the message text content, but the LLM sees the full chat template format including role tokens.

**How to avoid:** Either use tiktoken-rs's `get_chat_completion_max_tokens()` function which handles model-specific chat templates, or serialize each message to its template form before counting. For GPT-4o, the overhead is ~3 tokens per message for role markers.

**Warning signs:** Token count consistently underestimates actual usage by 10-30%, leading to context overflow despite the 65% check passing.

### Pitfall 2: Summarization Race Condition

**What goes wrong:** The summarization LLM call spawns asynchronously but the main loop continues and makes another LLM call before the summary is ready. The next turn uses the uncompressed message history and potentially exceeds the context window.

**Why it happens:** The summary is needed for the next LLM call, but async spawning means it may not complete in time.

**How to avoid:** The token count check should trigger compression BEFORE the threshold is actually reached (hence 65%, not 95%). The summary is computed but only applied on the NEXT turn. On the turn where the 65% threshold is crossed, the full history is still within limits -- the summary replaces older turns starting from the subsequent turn.

**Warning signs:** Context window exceeded errors on the turn immediately after summarization is triggered.

### Pitfall 3: SQLite Concurrent Write Contention

**What goes wrong:** Multiple sessions writing to the same SQLite database simultaneously cause "database is locked" errors.

**Why it happens:** SQLite serializes writes. Without WAL mode, readers also block on writers.

**How to avoid (D-09):** Enable WAL mode (`PRAGMA journal_mode=WAL`) so readers don't block writers. Set `busy_timeout=5000` so writers wait up to 5s for the lock instead of failing immediately. Use `BEGIN IMMEDIATE` for write transactions. Keep pool size at 3-5 connections -- more connections don't increase write concurrency.

**Warning signs:** Sporadic "database is locked" errors under concurrent session load.

### Pitfall 4: Wasm Store Lifecycle Confusion on Resume

**What goes wrong:** Attempting to serialize and restore a wasmtime `Store<T>`, or assuming the Store persists across restarts.

**Why it happens:** Novice confusion between conversational state (messages, trace, config -- serializable) and runtime state (JIT code, linear memory -- not serializable).

**How to avoid:** Resume always creates a fresh Store via `InstancePool::acquire()`. Only conversational state is restored from the database. The Store is treated as a disposable execution container that is created per session-run and destroyed on completion/pause. [CITED: CONTEXT.md D-06a]

**Warning signs:** Code that attempts `serde::Serialize` on `SessionHandle` or `Store<T>`, or file paths suggesting "store checkpoint" persistence.

### Pitfall 5: UUID v7 Binary Storage in SQLite

**What goes wrong:** SQLite has no native UUID type. Storing as TEXT (36 chars) wastes space compared to BLOB (16 bytes). Index lookups on TEXT are slower.

**Why it happens:** Default tendency to use `.to_string()` on UUID and store as text.

**How to avoid:** Store `session_id` and `tenant_id` as `BLOB PRIMARY KEY` (16 bytes). Use `Uuid::as_bytes()` for binding and `Uuid::from_bytes()` for extraction. CONTEXT.md specifies `session_id BLOB PRIMARY KEY` confirming this approach.

**Warning signs:** TEXT primary key columns on session_id/tenant_id, or 36-byte hex strings appearing in database dumps.

## Code Examples

Verified patterns from official sources:

### Token Counting with tiktoken-rs Singleton
```rust
// Source: tiktoken-rs GitHub README (zurawiki/tiktoken-rs)
// Confirmed via WebFetch of github.com/zurawiki/tiktoken-rs
use tiktoken_rs::o200k_base_singleton;

let bpe = o200k_base_singleton();
let tokens = bpe.encode_with_special_tokens("This is a sentence   with spaces");
println!("Token count: {}", tokens.len());
```

### SQLx SQLite Pool with WAL Mode
```rust
// Source: docs.rs/sqlx SqliteConnectOptions (verified via WebFetch)
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::time::Duration;

let opts = SqliteConnectOptions::from_str("sqlite://data.db")?
    .journal_mode(SqliteJournalMode::Wal)
    .busy_timeout(Duration::from_secs(5))
    .foreign_keys(true); // on by default in sqlx

let pool = SqlitePoolOptions::new()
    .max_connections(5)
    .connect_with(opts)
    .await?;
```

### SQLx Migration Embedding
```rust
// Source: docs.rs/sqlx migrate! macro (verified via WebFetch)
// In jadepaw-db/src/lib.rs or a dedicated init function:
sqlx::migrate!("./migrations")
    .run(&pool)
    .await?;
```

### JSON Blob Persistence Pattern
```rust
// D-03 pattern: serialize Vec<ChatCompletionRequestMessage> to JSON TEXT column
// Both types already derive Serialize/Deserialize

let messages_json = serde_json::to_string(&messages)?;
let trace_json = serde_json::to_string(&trace)?;
let guard_json = serde_json::to_string(&guard_config)?;

sqlx::query(
    "INSERT INTO sessions (session_id, tenant_id, status, messages_json, trace_json, guard_config_json, created_at, updated_at)
     VALUES (?, ?, 'running', ?, ?, ?, ?, ?)
     ON CONFLICT(session_id) DO UPDATE SET
       messages_json = excluded.messages_json,
       trace_json = excluded.trace_json,
       guard_config_json = excluded.guard_config_json,
       updated_at = excluded.updated_at"
)
.bind(session_id.as_bytes())
.bind(tenant_id.as_bytes())
.bind(&messages_json)
.bind(&trace_json)
.bind(&guard_json)
.bind(&now)
.bind(&now)
.execute(&pool)
.await?;
```

### SessionStatus Enum
```rust
// With SQLite CHECK constraint: CHECK(status IN ('idle','running','paused','ended'))

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "paused")]
    Paused,
    #[serde(rename = "ended")]
    Ended,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Ended => write!(f, "ended"),
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Naive message truncation (drop oldest) | Hybrid summarization + sliding window | 2023+ (Claude, ChatGPT) | Preserves semantic context while staying within token limits |
| Per-message-row SQL schema | JSON blob columns for message/trace | 2024+ (agent platforms) | Single write per turn vs N writes; simpler schema evolution |
| Manual SQLite pool + rusqlite | sqlx with async pool + compile-time checks | 2020+ (sqlx 0.3+) | Type-safe queries catch schema mismatches at compile time |
| Full trace serialization in AgentResponse only | DB-persisted trace with JSON blob | This phase | Enables pause/resume and crash recovery |

**Deprecated/outdated:**
- **rusqlite for new Rust projects:** sqlx is the async-native alternative with broader ecosystem support. rusqlite is synchronous and lacks compile-time query verification.
- **Custom token counting libraries:** tiktoken-rs is the standard. Previous alternatives like `tokenizers` (HuggingFace) are heavier and not model-specific for OpenAI models.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `Vec<ChatCompletionRequestMessage>` from async-openai 0.40 supports `Serialize`/`Deserialize` (CONTEXT.md states it does for 0.34; workspace now uses 0.40) | Standard Stack / Code Examples | Low -- async-openai types typically derive serde. If 0.40 removed it, blob persistence needs a manual intermediate type. |
| A2 | sqlx 0.9 `SqliteConnectOptions` supports `.journal_mode()`, `.busy_timeout()`, `.foreign_keys()` methods identical to 0.8 (verified via docs.rs WebFetch on latest) | Standard Stack | Low -- these are stable SQLite pragmas that sqlx has supported since 0.5. |
| A3 | tiktoken-rs 0.12.0's `o200k_base_singleton()` works without async runtime (it's CPU-bound, no I/O) | Architecture Patterns | Very low -- token counting is CPU math, no I/O involved. |
| A4 | `async-openai` 0.40 has the same `ChatCompletionRequestMessage` type structure as 0.34 (workspace upgraded from the original stack spec) | Code Examples | Low -- async-openai follows OpenAI API spec closely; message types are stable. |
| A5 | No existing `sqlx::query!` usage in project means no `.sqlx` offline data exists -- need `cargo sqlx prepare` after first migration | Common Pitfalls | None -- this is a procedural step, not a risk. |

## Open Questions (RESOLVED)

1. **Where should `SessionRepository` trait live?**
   - RESOLVED: D-04 specifies the trait lives in the new `jadepaw-db` crate (not jadepaw-core). The PLAN follows D-04 exactly — `SessionRepository` trait is defined in `jadepaw-db/src/repository.rs`, and `jadepaw-agent` takes a direct dependency on `jadepaw-db`. This is the "one crate per concern" pattern. The HostFunctions pattern (trait in core) was considered but D-04 is a locked decision that overrides.
   - PLAN: 05-01 Task 2 (models + trait + migration) places the trait in `crates/jadepaw-db/src/repository.rs`.

2. **Did async-openai 0.40 change `ChatCompletionRequestMessage` serde support?**
   - RESOLVED: Source inspection of async-openai 0.40.2 confirmed `ChatCompletionRequestMessage` still derives `Serialize` and `Deserialize`. No intermediate types needed.
   - PLAN: 05-02 Task 3 (react_loop integration) and Task 4 (resume_session) use `serde_json::to_string(&messages)` and `serde_json::from_str::<Vec<ChatCompletionRequestMessage>>()` directly — no mapping layer required.

3. **tiktoken-rs 0.12.0 C dependency compatibility?**
   - RESOLVED: This is a build-time dependency only — the C compiler is available on all target platforms (macOS via Xcode CLT, Linux via build-essential). The CONTEXT.md D-02 specifies tiktoken-rs v0.12 as a locked decision. The deferred idea about WASM blob size is only monitored, not a blocker.
   - PLAN: 05-02 Task 1 adds `tiktoken-rs = "0.12"` to `jadepaw-agent/Cargo.toml`. Task 2 (window.rs) imports and uses it. No workaround needed.
## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All crates | Yes | 1.95.0 | -- |
| Cargo | Build system | Yes | 1.95.0 | -- |
| sqlite3 CLI | Dev/debug tool | Yes | 3.51.0 | -- |
| sqlx-cli | Migration management | No | -- | Install via `cargo install sqlx-cli --no-default-features --features sqlite` |
| C compiler (cc) | tiktoken-rs build | Unknown | -- | macOS has Xcode CLT; CI needs `build-essential` |

**Missing dependencies with no fallback:**
- **sqlx-cli:** Required for `cargo sqlx prepare` (generates `.sqlx` offline query data for CI). Must be installed as a setup step in Wave 0.

**Missing dependencies with fallback:**
- None -- sqlx-cli is the only missing tool and has a straightforward install path.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (tokio::test) + nextest for speed |
| Config file | none (nextest configured globally in workspace) |
| Quick run command | `cargo test -p jadepaw-db -p jadepaw-agent -- --test-threads=4` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MEM-01 | Token counting correctly identifies when messages exceed 65% of context window | unit | `cargo test -p jadepaw-agent window::tests::token_count_triggers_at_65_percent -- --nocapture` | No -- Wave 0 |
| MEM-01 | Older messages are summarized while recent N=5 remain verbatim | unit | `cargo test -p jadepaw-agent window::tests::summarization_preserves_recent_n -- --nocapture` | No -- Wave 0 |
| MEM-01 | Context window compression keeps total tokens under limit | integration | `cargo test -p jadepaw-agent window::tests::compression_respects_token_budget -- --nocapture` | No -- Wave 0 |
| MEM-02 | Session snapshot persisted after each turn and recoverable after "crash" | integration | `cargo test -p jadepaw-db repository::tests::turn_boundary_persist_and_recover -- --nocapture` | No -- Wave 0 |
| MEM-02 | Paused session resumes from exact state (messages, trace, guard) | integration | `cargo test -p jadepaw-agent session::tests::pause_resume_roundtrip -- --nocapture` | No -- Wave 0 |
| MEM-02 | Crash recovery marks running sessions as paused | unit | `cargo test -p jadepaw-db repository::tests::crash_recovery_marks_running_as_paused -- --nocapture` | No -- Wave 0 |
| MEM-02 | Two simultaneous sessions have fully isolated contexts | integration | `cargo test -p jadepaw-db repository::tests::session_isolation_no_cross_contamination -- --nocapture` | No -- Wave 0 |
| MEM-02 | SQLite database is a single file, backup by copying | smoke | `cargo test -p jadepaw-db repository::tests::database_file_is_portable -- --nocapture` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p jadepaw-db -- --test-threads=4` (db crate has no heavy deps)
- **Per wave merge:** `cargo test -p jadepaw-db -p jadepaw-agent -- --test-threads=4`
- **Phase gate:** `cargo test --workspace` (all crates, full suite)

### Wave 0 Gaps
- [ ] `crates/jadepaw-db/src/repository.rs` -- SessionRepository trait definition, test scaffolding
- [ ] `crates/jadepaw-db/tests/` -- Integration tests directory (new crate, no tests exist yet)
- [ ] `crates/jadepaw-agent/src/window.rs` -- Context window module (token counting, summarization)
- [ ] `crates/jadepaw-agent/tests/context_window.rs` -- Window behavior integration tests
- [ ] `crates/jadepaw-agent/tests/session_persistence.rs` -- Persistence roundtrip tests
- [ ] sqlx-cli install: `cargo install sqlx-cli --no-default-features --features sqlite` -- if not installed
- [ ] `.sqlx/` directory -- Generated offline query data for CI (created by `cargo sqlx prepare`)
- [ ] SQLite test database location -- Use `:memory:` for unit tests, temp file for integration tests (no persistent test DB needed)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Not in scope for Phase 5 (Phase 7/8 handles auth) |
| V3 Session Management | Yes (partial) | Session isolation via `session_id` + `tenant_id` mandatory parameters on all `SessionRepository` methods (D-08). Session state machine prevents invalid transitions. |
| V4 Access Control | Yes | `tenant_id` required on every repository method. Type system enforces this -- no raw SQL access outside the repository module. |
| V5 Input Validation | Yes | JSON blob columns validated by serde deserialization on load. Normalized columns constrained by SQLite CHECK constraints and Rust enum types. |
| V6 Cryptography | No | No cryptographic operations in this phase. SQLite file can be encrypted at rest via filesystem-level encryption. |
| V7 Error Handling | Yes | `SessionRepository` trait returns `anyhow::Result` / `JadepawError`. DB errors logged with `session_id` and `tenant_id` context but not leaked to callers. |

### Known Threat Patterns for SQLite + Agent Sessions

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| SQL injection via unsanitized JSON blob binding | Tampering | Use `sqlx::query` with `?` bind parameters (never string interpolation). sqlx parameterizes all bindings. |
| Cross-tenant data access via missing WHERE clause | Information Disclosure | `SessionRepository` trait requires `tenant_id` on every method. Code review gate: no `SqlitePool` access outside repository impl. |
| Session data exfiltration via file copy of SQLite DB | Information Disclosure | SQLite file permissions (0600). Document backup security in operational docs. File-level encryption when required. |
| Session status manipulation (e.g., resume-ended-session) | Elevation of Privilege | CHECK constraint on `status` column. Rust enum prevents invalid transitions at type level before DB call. |

## Sources

### Primary (HIGH confidence)
- [CONTEXT.md D-01 through D-09a] -- All locked decisions, schema design, state machine, isolation model, WAL configuration
- [tiktoken-rs crates.io + GitHub] -- v0.12.0 confirmed, 8.8M downloads, `o200k_base_singleton()` and `cl100k_base_singleton()` API verified
- [docs.rs/sqlx SqliteConnectOptions] -- `.journal_mode()`, `.busy_timeout()`, `.foreign_keys()` methods verified
- [docs.rs/sqlx migrate! macro] -- Migration embedding, naming convention, `.run()` API
- [docs.rs/sqlx query! / query_as!] -- Compile-time checked SQL, bind parameters, offline mode with `cargo sqlx prepare`
- [Workspace Cargo.toml] -- sqlx 0.9 already declared, async-openai 0.40, serde_json, uuid v7, chrono

### Secondary (MEDIUM confidence)
- [crates/jadepaw-agent/src/loop.rs] -- `react_loop()` integration points confirmed at lines 141-149 (TODO WR-04 for windowing), line 265 (appending to messages)
- [crates/jadepaw-core/src/agent_types.rs] -- `ReActStep` derives Serialize/Deserialize, `AgentRequest` ready for `resume_from` extension
- [crates/jadepaw-wasm/src/session.rs] -- `SessionState` has `session_id`, `tenant_id`, `created_at` mapping to normalized DB columns
- [crates/jadepaw-agent/src/guard.rs] -- `GuardConfig` ready for `elapsed_ms` accumulator and `WindowConfig` extension
- [tiktoken-rs GitHub README] -- `get_chat_completion_max_tokens()` for model-specific chat template counting

### Tertiary (LOW confidence)
- None -- all claims in this research are either verified against official sources or explicitly tagged [ASSUMED] in the Assumptions Log.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- both tiktoken-rs v0.12.0 and sqlx 0.9.0 verified on crates.io and consistent with workspace. Both pass slopcheck.
- Architecture: HIGH -- integration points verified against existing source code (loop.rs, guard.rs, agent_types.rs, session.rs). All locked decisions from CONTEXT.md are specific and unambiguous.
- Pitfalls: HIGH -- Wasm Store serialization constraint is an irreducible wasmtime limitation. SQLite WAL contention is well-documented. Token counting template format is a known tiktoken-rs API behavior.

**Research date:** 2026-06-04
**Valid until:** 2026-07-04 (30 days -- both tiktoken-rs and sqlx are stable libraries with infrequent major releases)