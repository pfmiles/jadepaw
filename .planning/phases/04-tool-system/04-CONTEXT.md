# Phase 4: Tool System - Context

**Gathered:** 2026-06-03
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase bridges the ReAct loop's `Action` step with real tool execution, replacing the Phase 3 placeholder observation. Tools are registered through an MCP-compatible interface and dispatched via a host-side registry with capability-aware authorization. The `http_request` stub from Phase 2 is implemented with real HTTP logic and SSRF protection. Phase 2's Wasm host functions (`file_read`, `file_write`) remain as the sandboxed backend — the `Tool` trait wraps them for agent-level dispatch.

**Success Criteria (from ROADMAP.md):**
1. A developer registers a file_read tool with the agent, sends a query like "read the file /data/notes.txt and summarize it", and the agent calls the tool, reads the file, and returns the summary
2. A developer registers an http_request tool, and the agent can fetch a public URL and process the response content
3. Tools are registered through an MCP-compatible interface — a tool implemented for Claude Code should be usable by jadepaw with minimal adaptation
4. A tool call that fails (e.g., file not found, HTTP 500) is reported back to the agent with structured error information, and the agent can adapt its next action accordingly

**Requirements covered:** AGENT-02, REQ-SECURITY-004 (Phase 2 stub → full impl)

</domain>

<decisions>
## Implementation Decisions

### Tool Abstraction & Execution Path
- **D-01:** Registry + capability-aware dispatch. A `Tool` trait lives in `jadepaw-core` defining `name()`, `description()`, `input_schema()`, `call(args, session_handle) -> Result<ToolResult>`. A `ToolRegistry` in `jadepaw-agent` holds `HashMap<ToolId, Arc<dyn Tool>>` and is the single dispatch point for the ReAct loop. `can_call_tool()` on `SessionState` (Phase 2) is the authoritative capability gate — checked once at registry dispatch time.
- **D-01a:** `Tool` trait and `HostFunctions` trait have separate concerns: `Tool` = agent-level abstraction (what the ReAct loop dispatches), `HostFunctions` = Wasm-level contract (guest-host communication). Wasm host functions (`file_read`, `file_write`, `http_request`) are wrapped as `Tool` impls — the agent loop dispatches through the Registry, which internally uses `SessionHandle` to reach the Wasm backend when sandbox isolation is needed.
- **D-01b:** The Wasm Linker's host functions remain available for direct guest calls (preserving the Phase 2 sandbox contract), but the agent loop does NOT dispatch tools through the guest. Guest `select_tool` export becomes advisory (recommended but not enforced).

### MCP Compatibility Scope
- **D-02:** MCP-compatible wire format, implemented internally. The `ToolRegistry` provides MCP-style `tools/list` (returns `Vec<{name, description, inputSchema}>`) and `tools/call` (returns `{content, isError}`) interfaces. Tool definitions use MCP's JSON Schema format for `inputSchema`. No external MCP server connection in Phase 4 — tools are registered in-process as Rust types. The wire format alignment enables future externalization to full MCP client/server in Phase 6+.
- **D-02a:** No new external dependencies for MCP. `serde_json::Value` (already in dep tree) carries `inputSchema`. JSON-RPC handling is minimal — just the two methods needed for tool discovery and invocation.

### HTTP Tool Implementation
- **D-03:** Domain whitelist + IP-layer check. The existing `can_network_to` domain whitelist (Phase 2) is the primary defense. Added IP-layer validation: resolve hostname via `tokio::net::lookup_host`, reject any resolved IP in private/loopback/link-local/multicast ranges using `std::net` (zero new dependencies). DNS rebinding is accepted as a documented known risk for MVP.
- **D-03a:** Use `reqwest` (already transitive via async-openai) added as a direct dependency in `jadepaw-wasm`. Support GET/POST/PUT/PATCH/DELETE methods. Follow at most 1 redirect via custom `redirect::Policy`. Buffer response body with 1MB cap. Enforce 30s timeout via `tokio::time::timeout`. TLS certificate verification enabled by default.
- **D-03b:** No rate limiting in Phase 4 MVP — the `TenantQuotaLimiter` from Phase 2 provides per-tenant aggregate resource budgeting. Per-tool rate limiting deferred to post-MVP.

### Tool Error Reporting
- **D-04:** Structured `ToolResult` enum in `jadepaw-core` with variants `Ok { data: serde_json::Value }` and `Error { code: String, message: String, retryable: bool }`. Maps to MCP's two-tier error model: protocol errors (capability denied, path validation failure — LLM cannot fix) vs tool execution errors (file not found, HTTP 500 — LLM can adapt).
- **D-04a:** `ReActStep::Observation` gains an `is_error: bool` field (default `false`). The `result` string is formatted from structured error data for LLM actionability: `"Error: file not found at '/data/notes.txt' (code: NOT_FOUND). Suggested: check the file path exists and try again."`
- **D-04b:** Host functions at the Wasm FFI boundary still return `i32` (-1 on error). The `Tool` trait impl wraps this into `ToolResult::Error` with structured context for the agent.

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Phase 2 Output (tool foundation)
- `crates/jadepaw-wasm/src/host/mod.rs` — Host function module structure, namespace `"jadepaw"`
- `crates/jadepaw-wasm/src/host/filesystem.rs` — `file_read_host_fn`, `file_write_host_fn` (capability-gated, path-validated)
- `crates/jadepaw-wasm/src/host/network.rs` — `http_request_host_fn` stub (Phase 4 TODO at line 120-131), `extract_host_from_url`
- `crates/jadepaw-wasm/src/linker.rs` — `create_linker`, `register_host_functions` (Linker registration under `"jadepaw"` namespace)
- `crates/jadepaw-wasm/src/capability/mod.rs` — `can_read_file`, `can_write_file`, `can_call_tool`, `can_access_domain` on `SessionState`
- `crates/jadepaw-wasm/src/session.rs` — `SessionState`, `SessionHandle` with `store()`, `store_mut()`, `instance()` accessors
- `crates/jadepaw-core/src/host_functions.rs` — `HostFunctions` trait (additive-only, missing `http_request` — must be added)
- `crates/jadepaw-core/src/types.rs` — `ToolId`, `SessionId`, `TenantId`
- `crates/jadepaw-core/src/capabilities.rs` — `InstanceCapabilities` (includes `can_exec_tools: Vec<ToolId>`)

### Phase 3 Output (agent loop integration)
- `crates/jadepaw-agent/src/loop.rs` — `react_loop()` with placeholder `Observation` at line 220-228; `LlmDirective::Act { tool, args }` dispatch point
- `crates/jadepaw-agent/src/llm.rs` — `LlmDirective` enum, `stream_llm_response`, `parse_next_action`
- `crates/jadepaw-agent/src/guard.rs` — `GuardConfig`, termination guards
- `crates/jadepaw-agent/src/lib.rs` — Public API, `run_agent()`
- `crates/jadepaw-core/src/agent_types.rs` — `AgentRequest`, `AgentResponse`, `ReActStep` (Observation variant needs `is_error` field)

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` §Agent Core — AGENT-02 requirement
- `.planning/REQUIREMENTS.md` §Security & Isolation — REQ-SECURITY-004 (network access control, Phase 4 full impl)
- `.planning/ROADMAP.md` §Phase 4 — Phase goal, 4 success criteria, dependency on Phase 3
- `.planning/PROJECT.md` — Core constraints, key decisions, MCP-compatible tool protocol

### Prior Phase Context
- `.planning/phases/03-agent-runtime/03-CONTEXT.md` — ReAct loop architecture (D-01–D-04), LLM integration (D-05–D-07), AgentRequest/AgentResponse types (D-12–D-14)
- `.planning/phases/02-wasm-isolation-core/02-CONTEXT.md` — HostFunctions trait (D-01–D-03), InstancePool (D-04–D-06), ResourceLimiter chain (D-07–D-09a), Capability enforcement (D-10–D-12)

### Architecture & Design
- `docs/jadepaw_discussion.md` — Tool execution model, capability model, security model
- `.planning/notes/mvp-core-decisions.md` — MVP core decisions

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `jadepaw-core` crate — `ToolId`, `InstanceCapabilities` (with `can_exec_tools`), `HostFunctions` trait, `ReActStep::Observation` variant. Ready to receive `Tool` trait, `ToolResult` enum, `ToolDefinition` struct.
- `jadepaw-wasm` crate — `SessionState::can_call_tool()` already implemented and tested. `SessionHandle` provides `store()`, `store_mut()`, `instance()` accessors for Wasm host function invocation. `register_host_functions()` in `linker.rs` is the canonical registration point.
- `jadepaw-agent` crate — `react_loop()` in `loop.rs` line 190-228 is the exact integration point for tool dispatch. `LlmDirective::Act { tool, args }` already carries the parsed tool name and JSON args.
- `http_request_host_fn` in `network.rs` — Input validation (bounds-check, domain capability check) is already done. The stub body at line 120-131 is where Phase 4's HTTP logic drops in.
- Workspace `Cargo.toml` — `reqwest` is already transitive via async-openai. `serde_json` already available.

### Established Patterns
- **Types in core, impl downstream**: `Tool` trait + `ToolResult` in `jadepaw-core`, `ToolRegistry` in `jadepaw-agent`, Wasm-backed `Tool` impls in `jadepaw-wasm`.
- **Additive-only interfaces**: `HostFunctions` trait gains `http_request` method (additive, not breaking). `Tool` trait is new — no compatibility constraint yet.
- **Capability-gated before I/O**: Every host function checks `caller.data().can_*()` before side effects. `ToolRegistry::call_tool()` checks `can_call_tool()` before dispatch.
- **Defense-in-depth**: Capability check at Registry level (policy) + Wasm host function entry (security boundary). Redundancy is intentional and documented.
- **ResourceLimiter delegating chain**: Phase 4 does not extend the chain — HTTP timeout and body cap are implemented at the tool level, not the Wasm level.

### Integration Points
- Phase 5 (Session Memory): Tool results feed into conversation context; session persistence snapshots `ToolRegistry` state.
- Phase 6 (Skill System): Skills declare tool requirements by `ToolId`; `ToolRegistry` validates at skill load time. External MCP server support added in Phase 6+.
- Phase 7 (Web Chat UI): `ReActStep::Observation` with `is_error` field streams via SSE as named events.
- Phase 9 (Observability): Tool call spans nest under agent reasoning spans in the trace hierarchy.

</code_context>

<specifics>
## Specific Ideas

- `ToolRegistry` in `jadepaw-agent` uses `DashMap<ToolId, Arc<dyn Tool>>` for concurrent read access (same pattern as Phase 2's `InstancePool` session tracking).
- `ToolDefinition` struct matches MCP's `{ name: String, description: String, inputSchema: serde_json::Value }` — this is the canonical tool metadata format.
- `HostFunctions` trait in `jadepaw-core` gains `async fn http_request(&self, method: String, url: String, headers: HashMap<String, String>, body: Option<Vec<u8>>) -> Result<(u16, HashMap<String, String>, Vec<u8>)>` — returns status code, response headers, response body.
- `Observation` variant change: `Observation { result: String, is_error: bool }` — backward compatible if `is_error` defaults to `false`.
- HTTP SSRF blocked IP ranges: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `127.0.0.0/8`, `169.254.0.0/16`, `224.0.0.0/4`, `::1`, `fe80::/10`, `fc00::/7`. All checkable via `std::net::IpAddr` in Rust 1.85+.
- File tools (`file_read`, `file_write`) are registered in the Registry as `Tool` impls that internally call the Wasm host function via `SessionHandle` — the Wasm sandbox remains the security boundary for file I/O.

</specifics>

<deferred>
## Deferred Ideas

- **Full MCP client (rmcp)**: Connecting to external MCP server subprocesses — deferred to Phase 6+ when Skill authors need to bring their own MCP tools.
- **DNS rebinding defense (hickory-resolver or IP pinning)**: Accepted as known risk for MVP. Add when security audit or real-world attack evidence demands it.
- **Per-tool rate limiting**: `TenantQuotaLimiter` from Phase 2 covers aggregate budgets. Per-tool rate limiting is a future extension of the ResourceLimiter delegating chain.
- **Tool output streaming**: Buffered responses only for MVP. Streaming tool output (e.g., long HTTP downloads) deferred.
- **Tool timeout per-invocation**: Global HTTP timeout (30s) is sufficient for MVP. Per-tool configurable timeouts can be layered on later.

</deferred>

---

*Phase: 4-Tool System*
*Context gathered: 2026-06-03*