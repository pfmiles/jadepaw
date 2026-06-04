# Phase 5: Session Memory - Context

**Gathered:** 2026-06-04
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase adds in-session context management and SQLite-based persistence to the ReAct agent loop (Phase 3). Two layers: (1) automatic context window compression — when conversation history approaches the model's token limit, older turns are summarized rather than truncated; (2) session pause/resume — conversational state is persisted to SQLite at each turn boundary, enabling recovery from explicit pause, timeout, or crash. Sessions remain isolated at both the Wasm (Store-per-session) and database (SessionStore trait enforced) layers.

**Success Criteria (from ROADMAP.md):**
1. A user has a long conversation (>10 messages) and the agent remembers earlier context — when the conversation approaches the token limit, older messages are automatically compressed into a summary rather than truncated
2. A user pauses a session (or the system crashes), and after restarting, the user can resume the session from exactly where it left off — all conversation history, agent state, and pending actions are preserved
3. A user opens two sessions simultaneously — their contexts are fully isolated with no cross-contamination
4. Session data is stored in SQLite (single-file, zero-config), and the database can be backed up by copying the file

**Requirements covered:** MEM-01, MEM-02

</domain>

<decisions>
## Implementation Decisions

### Context Window Management
- **D-01:** Hybrid approach — summarize older turns, keep recent N turns verbatim (N configurable, default 5). The summary preserves tool names, action types, error status, and key findings while dropping verbose thought content. The compressed summary is injected as a system-prefix message, maintaining structure: system prompt + tool definitions + summary + recent N turns.
- **D-02:** Adaptive threshold — compression triggers when total tokens reach **65%** of the model's context window (user-specified empirical threshold from agent usage experience). Token counting uses `tiktoken-rs` with model-appropriate singleton (`o200k_base_singleton` for GPT-4o/4.1, `cl100k_base_singleton` for GPT-4/3.5-turbo).
- **D-02a:** Summarization itself runs asynchronously — the summarization LLM call happens outside the hot ReAct loop path, summarizing all turns older than the most recent N. This adds one extra LLM call per ~10-15 turns.
- **D-02b:** Token counting check runs before each LLM call in the ReAct loop (~10ms CPU overhead, negligible vs LLM latency). Not a separate background task — the check must be synchronous to block the next LLM call before context overflow.

### Persistence Schema & Storage
- **D-03:** Hybrid schema — normalized session metadata columns + JSON blob for message history and trace. Session metadata (`session_id` UUID v7, `tenant_id`, `status`, `created_at`, `termination_reason`, `guard_config`) in typed columns for SQL queryability. `Vec<ChatCompletionRequestMessage>` and `Vec<ReActStep>` stored as single JSON TEXT columns — both types already derive `Serialize`/`Deserialize`, so persistence is a single `serde_json::to_string` call, no per-row INSERT overhead.
- **D-04:** New `jadepaw-db` crate — follows the existing architectural pattern (`jadepaw-bus` for messaging, `jadepaw-wasm` for runtime). Exposes a `SessionRepository` trait with `save`, `load`, `list_by_tenant`, `delete` methods, backed by `SqlitePool` (single-node) with a clean migration path to `PgPool` (cluster). Keeps `jadepaw-agent` focused on the ReAct loop rather than database concerns.
- **D-05:** SQLx for database access — compile-time checked queries (`query!` / `query_as!`) for normalized metadata columns where typed fields matter; runtime `sqlx::query` with `serde_json::Value` binding for message/trace blob columns. `cargo sqlx prepare` workflow for CI.
- **D-05a:** Single migration file for Phase 5: `sessions` table with metadata columns + `messages_json` TEXT + `trace_json` TEXT + `guard_config_json` TEXT. Schema can be extended with additional tables (`messages`, `trace_steps`) via migrations in Phase 6/9 if query patterns demand it.

### Pause/Resume Lifecycle
- **D-06:** Full-state snapshot at turn boundaries — after each completed ReAct turn (thought → act → observe cycle finishes), persist: message history, execution trace, wall-clock accumulator, iteration counter, GuardConfig. On resume: re-acquire a fresh Wasm Store from InstancePool, reconstruct the agent loop at the correct turn, continue from the next turn — no replay of completed LLM calls.
- **D-06a:** Wasm Stores are NOT serialized — wasmtime's `Store` contains JIT-compiled code and PoolingAllocator linear memory that only exist in-process. Resume always creates a fresh Store from `InstancePre` and restores conversational state. This is an irreducible constraint, not a design choice.
- **D-06b:** Mid-turn crash behavior — the in-flight turn (LLM streaming or tool call) is lost. The last turn-boundary checkpoint is the recovery point. LLM streaming state is non-serializable; accept this as a documented limitation for MVP.
- **D-07:** Session status state machine: `idle` → `running` → `paused` → `running` → `ended`. Pause is triggered explicitly via API (`pause_session`). Crash recovery: on restart, scan for sessions with `status = 'running'` and mark them `paused` (the in-flight turn is lost per D-06b).
- **D-07a:** Wall-clock guard continuity — persist an `elapsed_ms` accumulator. On resume, the guard timer starts from the accumulated value, ensuring the total session wall-clock limit is enforced across pause/resume boundaries.

### Session Isolation
- **D-08:** Repository-layer enforcement — `SessionStore` trait all methods require `session_id` and `tenant_id` as mandatory parameters. The type system prevents forgetting the WHERE clause at call sites. No raw `SqlitePool` access outside the store module.
- **D-09:** SQLite WAL mode + `busy_timeout` — enable WAL for non-blocking concurrent reads (snapshot isolation), set `busy_timeout` (e.g., 5s) for orderly write serialization, `BEGIN IMMEDIATE` for write transactions. Connection pool size 3–5 max connections (SQLite is single-writer, but WAL readers don't block).
- **D-09a:** The Wasm layer (Store-per-session, ResourceLimiter, sandbox_root) remains the primary security boundary. The database layer is an internal persistence store — not an attack surface exposed to guest code. Repository-layer enforcement (D-08) provides compiler-enforced correctness; WAL mode (D-09) provides pragmatic concurrency.

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase 3 Output (agent loop foundation)
- `crates/jadepaw-agent/src/loop.rs` — `react_loop()` with unbounded `Vec<ChatCompletionRequestMessage>` accumulation (lines 141-142), TODO(WR-04) at lines 144-149 documenting message windowing approaches
- `crates/jadepaw-agent/src/llm.rs` — `build_initial_messages()`, `stream_llm_response()`, `REACT_SYSTEM_PROMPT`, `LlmDirective` enum
- `crates/jadepaw-agent/src/guard.rs` — `GuardConfig` (max_iterations, max_duration), `run_with_guard` (tokio::select! termination)
- `crates/jadepaw-agent/src/lib.rs` — `run_agent()` public API, `ToolRegistry` re-export
- `crates/jadepaw-core/src/agent_types.rs` — `AgentRequest` (session_id, user_message, context), `AgentResponse` (session_id, final_answer, trace: Vec<ReActStep>), `ReActStep` enum, `AgentTerminationReason` enum

### Phase 2 Output (Wasm isolation foundation)
- `crates/jadepaw-wasm/src/session.rs` — `SessionState` (session_id, tenant_id, capabilities, limits, created_at, sandbox_root), `SessionLimits`
- `crates/jadepaw-wasm/src/pool.rs` — `InstancePool` (Arc<InstancePre> + Semaphore + DashMap), `SessionHandle`
- `crates/jadepaw-wasm/src/engine.rs` — `EngineFactory`
- `crates/jadepaw-core/src/types.rs` — `SessionId`, `TenantId`
- `crates/jadepaw-core/src/capabilities.rs` — `InstanceCapabilities`

### Phase 4 Output (tool system)
- `crates/jadepaw-agent/src/tool_registry.rs` — `ToolRegistry` with capability-gated dispatch
- `crates/jadepaw-core/src/tool.rs` — `Tool` trait, `ToolResult`, `ToolDefinition`

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` §Memory — MEM-01 (context window management, auto-compression), MEM-02 (session persistence, pause/resume)
- `.planning/ROADMAP.md` §Phase 5 — Phase goal, 4 success criteria, dependency on Phase 3
- `.planning/PROJECT.md` — Core constraints, SQLite for single-node mode, SQLx 0.8+ as database library

### Prior Phase Context
- `.planning/phases/03-agent-runtime/03-CONTEXT.md` — ReAct loop architecture (D-01–D-04), LLM integration (D-05–D-07), termination guards (D-08–D-10), AgentRequest/AgentResponse types (D-12–D-14)
- `.planning/phases/04-tool-system/04-CONTEXT.md` — ToolRegistry dispatch, tool error reporting (D-04–D-04b)
- `.planning/phases/02-wasm-isolation-core/02-CONTEXT.md` — HostFunctions trait (D-01–D-03), InstancePool (D-04–D-06), ResourceLimiter chain (D-07–D-09a)

### Technology Reference
- `Cargo.toml` — workspace dependencies (sqlx 0.8+ already specified, async-openai 0.34.0, serde_json, tokio)
- `.planning/research/STACK.md` — Technology stack, SQLx patterns

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `jadepaw-agent/src/loop.rs` — `react_loop()` is the exact integration point for context window management (before the LLM call each turn) and turn-boundary checkpoint persistence (after each observation/thought cycle completes). The existing `messages: Vec<ChatCompletionRequestMessage>` is the data structure to window/compress.
- `jadepaw-agent/src/llm.rs` — `build_initial_messages()` is where initial context injection happens; `stream_llm_response()` takes `messages.clone()` — windowing happens on the `messages` vec before this call.
- `jadepaw-core/src/agent_types.rs` — `ReActStep` and `AgentTerminationReason` already derive `Serialize/Deserialize`. `AgentResponse.trace: Vec<ReActStep>` is the trace to persist. `AgentRequest` needs a `resume_from` field for the resume path.
- `jadepaw-wasm/src/session.rs` — `SessionState` has `session_id`, `tenant_id`, `capabilities`, `created_at` — these map directly to normalized DB columns.
- Workspace `Cargo.toml` — sqlx 0.8+ already specified. async-openai types (`ChatCompletionRequestMessage`) derive Serialize/Deserialize. No new crate dependencies beyond `tiktoken-rs` and sqlx features (`sqlite`, `chrono`, `uuid`, `runtime-tokio`).

### Established Patterns
- **Types in core, impl downstream**: `SessionRepository` trait in `jadepaw-db` (or `jadepaw-core` if keeping crate count minimal), SQLite impl in `jadepaw-db`, persistence logic in `jadepaw-agent`.
- **Store-per-session**: Existing lifecycle — `SessionState::new()` → `pool.acquire()` → `react_loop()` → `handle Drop`. Phase 5 inserts persistence between `react_loop` turns and before `handle Drop`.
- **Additive-only interfaces**: Pause/resume extends the agent loop API additively — new `pause_session()`, `resume_session()` functions alongside existing `run_agent()`.
- **Capability-gated before I/O**: Database access is host-side infrastructure, not guest-accessible. No Wasm boundary crossing for persistence.
- **Compile-time checked queries**: SQLx `query!` macro for normalized columns, following the same "correctness by construction" philosophy as Phase 2's capability checks.

### Integration Points
- Phase 6 (Skill System): Skills may need to query sessions by tenant; normalized `tenant_id` column enables this. Skill context injection may interact with the windowed message history.
- Phase 7 (Web Chat UI): SSE reconnection on session resume — consumer must handle reconnection and trace deduplication. The `AgentResponse.trace` contains the full history for replay.
- Phase 9 (Observability): Normalized `session_id` and `created_at` columns enable trace-to-OTel-span joins. Session status state machine feeds Prometheus gauges.

</code_context>

<specifics>
## Specific Ideas

- Token counting trigger threshold: **65%** of model context window — user-specified empirical threshold from agent usage. This is NOT a default to override; it's a locked decision.
- tiktoken-rs v0.12 for token counting — `o200k_base_singleton` for GPT-4o/4.1 models, `cl100k_base_singleton` for GPT-4/3.5-turbo. Singleton pattern avoids reloading the tokenizer per session.
- Recent N turns preserved verbatim — N = 5 default, configurable via `GuardConfig` extension or new `WindowConfig`.
- `SessionStore` trait methods: `save(session_id, messages_json, trace_json, guard_config_json, metadata)`, `load(session_id) -> SessionSnapshot`, `list_by_tenant(tenant_id) -> Vec<SessionSummary>`, `delete(session_id)`, `update_status(session_id, status)`.
- `SessionStatus` enum: `Idle`, `Running`, `Paused`, `Ended`. Stored as TEXT column with CHECK constraint.
- Connection pool config: `max_connections = 5`, WAL mode via `PRAGMA journal_mode=WAL`, `busy_timeout = 5000` via `PRAGMA busy_timeout=5000`, `foreign_keys = ON` via `PRAGMA foreign_keys=ON`. Applied in `after_connect` callback.
- Migration: single `01_create_sessions.sql` — `sessions` table with `session_id BLOB PRIMARY KEY`, `tenant_id BLOB NOT NULL`, `status TEXT NOT NULL DEFAULT 'idle'`, `created_at TEXT NOT NULL`, `updated_at TEXT NOT NULL`, `termination_reason_json TEXT`, `messages_json TEXT NOT NULL DEFAULT '[]'`, `trace_json TEXT NOT NULL DEFAULT '[]'`, `guard_config_json TEXT NOT NULL DEFAULT '{}'`. Index on `(tenant_id, created_at)`.

</specifics>

<deferred>
## Deferred Ideas

- **Long-term memory (MEM-03)**: Cross-session knowledge extraction and retrieval (vector DB, embedding pipeline) — v2.
- **Per-tool rate limiting in the persistence layer**: TenantQuotaLimiter already covers aggregate budgets. DB-layer rate limiting deferred.
- **Session migration across cluster nodes (MEM-04)**: Redis-based cross-node state transfer — v2 cluster mode.
- **tiktoken-rs WASM blob size (~5MB)**: Monitor build size impact. If problematic, consider token counting via API (e.g., OpenAI's token count endpoint) or a lighter counting library.

</deferred>

---

*Phase: 5-Session Memory*
*Context gathered: 2026-06-04*