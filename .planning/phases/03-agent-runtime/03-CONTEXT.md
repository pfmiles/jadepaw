# Phase 3: Agent Runtime - Context

**Gathered:** 2026-06-01
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase builds the intelligent reasoning layer on top of Phase 2's Wasm isolation infrastructure: a ReAct execution loop that receives user natural language input, calls LLM for reasoning, dispatches tool execution through the capability-gated Wasm sandbox, and streams responses back to the caller in real time via SSE. The loop runs on the host side (Rust) with guest export decision points that allow Wasm-based skills to customize tool selection, step evaluation, and stop conditions.

**Success Criteria (from ROADMAP.md):**
1. A user sends a natural language query and the agent responds with a reasoned answer — the agent can issue multiple think->act->observe cycles before responding
2. Response tokens stream to the caller in real time (SSE), not as a single batch after the full computation completes
3. An agent stuck in a loop is terminated after exceeding the max iteration limit (configurable, default 20), and the caller receives a graceful termination message explaining why
4. An agent that takes longer than the wall-clock timeout (configurable, default 5 minutes) is terminated with a timeout error message
5. The agent can be invoked programmatically (API call or test harness) and returns structured results including the final answer and execution trace

**Requirements covered:** AGENT-01, AGENT-03, AGENT-04

</domain>

<decisions>
## Implementation Decisions

### ReAct Loop Architecture (Hybrid Mode)
- **D-01:** The ReAct loop skeleton runs on the host side in `jadepaw-agent` crate (Rust). The host owns: LLM API calls, SSE streaming, tool execution dispatch (with Phase 2 capability checks), and termination guards (iteration limit + wall-clock timeout).
- **D-02:** Guest Wasm modules MAY export decision-point functions that the host loop calls at specific phases of the ReAct cycle. If a guest does not export a given function, the host falls back to default LLM-based behavior. This is the "SDK" for skill authors — a stable, additive-only interface.
- **D-03:** Initial guest export interface (Phase 3 MVP, expandable in Phase 4/6):
  - `evaluate_step(thought: String, observation: String) -> NextAction` — core decision function (continue thinking, act, or finish)
  - `select_tool(goal: String, available_tools: Vec<ToolDef>) -> ToolChoice` — custom tool selection logic (optional, defaults to LLM)
  - `should_continue(turn: u32, history_summary: String) -> bool` — custom stop condition (optional, defaults to LLM)
- **D-04:** Guest export interface follows the same additive-only policy as `HostFunctions` trait — functions may be added, never removed. The interface lives in `jadepaw-core` as trait definition(s), with default no-op implementations.

### LLM Integration
- **D-05:** Use async-openai's `Client<Box<dyn Config>>` directly in `jadepaw-agent` for Phase 3. The `Box<dyn Config>` pattern already handles multi-provider runtime dispatch (OpenAI, Azure, DeepSeek, Groq, Ollama, any OpenAI-compatible endpoint).
- **D-06:** Do NOT abstract an `LlmClient` trait in Phase 3. When Anthropic (non-OpenAI-compatible) or other providers become hard requirements, extract a trait based on real usage patterns — the "concrete first, abstract later" path validated by Phase 2.
- **D-07:** SSE token streaming from LLM to caller: async-openai's `ChatCompletionStream` → tokio channel → axum SSE response. No Wasm boundary crossing for token streaming — the loop runs on the host, so tokens flow directly through Rust async primitives.

### Termination Protection
- **D-08:** Host-level termination via `tokio::select!` in `jadepaw-agent/src/guard.rs`. Three futures race: agent loop completion, iteration counter (default 20), wall-clock timeout (default 5 minutes). First to trigger cancels the others.
- **D-09:** `JadepawError` in `jadepaw-core` gains an `AgentTerminationReason` enum with variants: `MaxIterationsReached { iter: u32, max: u32 }`, `WallClockTimeout { elapsed: Duration, max: Duration }`, `WasmTrap { reason: String, turn: u32 }`.
- **D-10:** Phase 2 Wasm-level protection (fuel 1M/turn, epoch ~1ms, 64MB hard cap) remains unchanged and covers single-turn instruction explosion. Host guards cover cross-turn iteration and hang protection. Layered: Wasm = security boundary, Host = policy boundary.
- **D-11:** Per-turn LLM/tool call timeout (`tokio::time::timeout` wrapping individual calls) is deferred — can be added as a layer on top of D-08 if profiling reveals LLM hang scenarios not covered by the global timeout.

### Invocation API
- **D-12:** Request/response types live in `jadepaw-core`: `AgentRequest` (session_id, user_message, context), `AgentResponse` (final_answer, trace), `ReActStep` enum (Thought, Action, Observation, Error). Pure data structures, serde-serializable, no wasmtime dependency.
- **D-13:** The agent execution interface is a function in `jadepaw-agent` (not a trait in core): `async fn run_agent(req: AgentRequest, pool: Arc<InstancePool>, llm: Client<Box<dyn Config>>) -> Result<AgentResponse>`. No trait abstraction needed at MVP — the function signature is the contract.
- **D-14:** SSE event mapping for streaming: each `ReActStep` emitted as a named SSE event (`event: thought`, `event: action`, `event: observation`), with `event: done` carrying the final answer and aggregated execution trace. This maps directly to Phase 7's HTMX SSE extension.

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase 2 Output (this phase's foundation)
- `crates/jadepaw-core/src/host_functions.rs` — HostFunctions trait (guest-host contract, additive-only pattern to follow)
- `crates/jadepaw-core/src/types.rs` — SessionId, TenantId, ToolId
- `crates/jadepaw-core/src/capabilities.rs` — InstanceCapabilities, PathPattern, DomainPattern
- `crates/jadepaw-core/src/error.rs` — JadepawError enum (extend with AgentTerminationReason)
- `crates/jadepaw-wasm/src/pool.rs` — InstancePool, SessionHandle (acquire/release lifecycle)
- `crates/jadepaw-wasm/src/session.rs` — SessionState, SessionLimits (Store<T> data)
- `crates/jadepaw-wasm/src/engine.rs` — EngineFactory
- `crates/jadepaw-wasm/src/limits/` — InstanceHardLimiter, TenantQuotaLimiter (delegating chain)

### Architecture & Design
- `docs/jadepaw_discussion.md` — Wasm isolation model, Agent Loop design (Section 6.1), security model
- `docs/arch.mermaid` — Architecture diagram
- `.planning/notes/mvp-core-decisions.md` — MVP core decisions, Agent Loop discussion

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` §Agent Core — AGENT-01, AGENT-03, AGENT-04 requirements
- `.planning/ROADMAP.md` §Phase 3 — Phase goal, 5 success criteria, dependency on Phase 2
- `.planning/PROJECT.md` — Core constraints, key decisions (Hybrid Agent Loop Pending, async-openai Box<dyn Config>)

### Prior Phase Context
- `.planning/phases/02-wasm-isolation-core/02-CONTEXT.md` — HostFunctions trait design (D-01–D-03), InstancePool design (D-04–D-06), ResourceLimiter chain (D-07–D-09a), Capability enforcement (D-10–D-12), guest-host FFI decisions
- `.planning/phases/01-project-foundation/01-CONTEXT.md` — Crate structure (D-01, D-02), dependency graph, feature flag strategy

### Research
- `.planning/research/STACK.md` — Technology stack, async-openai streaming patterns
- `.planning/research/ARCHITECTURE.md` — Per-crate responsibilities

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `jadepaw-core` crate — Ready to receive `AgentRequest`, `AgentResponse`, `ReActStep` types, and `AgentTerminationReason` enum. Follows the same pattern as existing `HostFunctions` trait and `InstanceCapabilities` struct (types in core, impl downstream).
- `jadepaw-wasm` crate — `InstancePool::acquire()` returns `SessionHandle` with `store()`, `store_mut()`, `instance()` accessors. Phase 3's agent loop acquires a handle, calls guest exports via `instance()`, and the handle's `Drop` cleans up Store and releases the semaphore permit.
- `jadepaw-agent` crate — Currently placeholder (lib.rs doc comments only). Ready for: `loop.rs` (ReAct orchestrator), `llm.rs` (async-openai integration), `guard.rs` (termination protection), `stream.rs` (SSE token relay).
- Workspace `Cargo.toml` — async-openai 0.34.0 already in workspace dependencies. jadepaw-agent already depends on jadepaw-core, jadepaw-wasm, tokio.

### Established Patterns
- **Additive-only interfaces**: HostFunctions trait in jadepaw-core is the model for guest export interfaces (D-03).
- **Types in core, impl in downstream**: HostFunctions trait + InstanceCapabilities in core, wasmtime impl in jadepaw-wasm. Agent types follow the same split.
- **Store-per-session**: Phase 2's SessionState/Store model means the agent loop gets exclusive access to a Wasm Store for the session duration.
- **Capability-gated before I/O**: Every host function checks `caller.data().capabilities.can_*()` before side effects. Agent loop's tool dispatch must follow the same pattern.
- **ResourceLimiter delegating chain**: InstanceHardLimiter → TenantQuotaLimiter pattern is extensible — Phase 3's termination guard is an ADDITION to the host-side policy layer, not a modification to the Wasm security layer.

### Integration Points
- Phase 4 (Tool System): The tool registry and MCP protocol adapter register tools that the agent loop discovers and calls. Guest export `select_tool` will reference these registered tools.
- Phase 5 (Session Memory): Context window management feeds into the agent loop's history; session persistence snapshots AgentSession state.
- Phase 6 (Skill System): Skills compile to Wasm modules that implement the guest export decision-point functions defined in D-03.
- Phase 7 (Web Chat UI): axum SSE endpoint consumes the tokio channel from the agent loop, mapping ReActStep events to SSE named events.
- `jadepaw-bus` crate: May be used for agent-to-agent communication in future multi-agent scenarios, but not required for Phase 3 MVP.

</code_context>

<specifics>
## Specific Ideas

- Guest Wasm modules implement decision-point functions as wasm exports with well-known names (e.g., `evaluate_step`, `select_tool`). If absent, host uses LLM-based defaults — progressive enhancement.
- The ReAct loop's "think" phase calls LLM with conversation history + system prompt; "act" phase dispatches tool calls through Phase 2's capability checks; "observe" phase feeds tool results back into context.
- SSE streaming path: `ChatCompletionStream` (async-openai) → `tokio::sync::mpsc` channel → axum `Sse` response. Each delta token emitted as `event: token`, complete thoughts as `event: thought`.
- Agent execution trace (`Vec<ReActStep>`) is accumulated during the loop and emitted both incrementally (SSE events) and in the final `AgentResponse`.
- Fuel reset per turn: `store.set_fuel(1_000_000)` at the start of each ReAct iteration, so the 1M fuel budget is per-turn, not per-session.

</specifics>

<deferred>
## Deferred Ideas

- **Pure guest-side loop (full Wasm autonomy)**: Deferred to post-Phase 6 when JIT harness and Skill compiler are mature. The hybrid mode's guest export interface is designed to evolve toward full guest-side execution by adding more decision points over time.
- **LlmClient trait abstraction**: Deferred until Anthropic or other non-OpenAI-compatible providers become hard requirements. Current async-openai `Box<dyn Config>` covers all Phase 3 needs.
- **Per-turn LLM/tool timeout**: Deferred — global timeout (5 min) + iteration limit (20) is sufficient for MVP. Per-turn timeout can be added as a layer on top of the existing guard without architectural changes.

</deferred>

---

*Phase: 3-Agent Runtime*
*Context gathered: 2026-06-01*