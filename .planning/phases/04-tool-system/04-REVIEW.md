---
phase: 04-tool-system
reviewed: 2026-06-03T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-agent/src/tool_registry.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/tool.rs
  - crates/jadepaw-core/tests/agent_types.rs
  - crates/jadepaw-core/tests/host_functions.rs
  - crates/jadepaw-wasm/Cargo.toml
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/mod.rs
findings:
  critical: 2
  warning: 5
  info: 2
  total: 9
status: issues_found
---

# Phase 4: Code Review Report

**Reviewed:** 2026-06-03T00:00:00Z
**Depth:** standard
**Files Reviewed:** 20
**Status:** issues_found

## Summary

20 source files across 3 crates (jadepaw-agent, jadepaw-core, jadepaw-wasm) implementing the Phase 4 Tool System. Review covered tool registry, ReAct loop with tool dispatch, SSE streaming, tool trait implementations (file_read, file_write, http_request), LLM integration, and termination guard. 2 blocker-level issues found that could cause silent data loss; 5 warnings around missing domain capability check, message history truncation, and security defense gaps.

## Critical Issues

### CR-01: `HttpRequestTool` missing domain capability check (authorization bypass)

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:216-376`
**Issue:** The entire `call()` method of `HttpRequestTool` performs scheme validation, method validation, SSRF IP-layer check, and HTTP execution -- but never checks `SessionState::can_access_domain()`. This means the domain whitelist capability (T-02-10) is completely bypassed for agent-dispatch HTTP requests. The host function path (`http_request_host_fn` in `host/network.rs:103-112`) correctly performs this check, but the `Tool` trait path has no session state reference to check against, making the domain whitelist effective only for Wasm-guest-initiated HTTP calls and entirely absent for agent-dispatched HTTP tool calls.

Note: The module doc comment at line 9 claims "Domain whitelist check (via `SessionState::can_access_domain`)" as defense layer 2, but this check is never actually performed anywhere in the `HttpRequestTool::call()` body.

**Fix:** The `HttpRequestTool` needs access to a session's capability state. Either: (1) Accept `session_id` and look up capability from the session store within `call()`, or (2) restructure the tool to accept an `Arc<InstanceCapabilities>` at construction time and check `can_access_domain` before the SSRF IP check. The `tool_registry::call_tool()` API already receives a `&SessionHandle` and returns results after checking `can_call_tool()` -- extend the capability gate to also check domain access in the `ToolRegistry` or provide the session capabilities to the tool.

### CR-02: `LoopErrorKind::MaxIterations` reports identical values for `iter` and `max` in final return

**File:** `crates/jadepaw-agent/src/loop.rs:268-271`
**Issue:** When the loop exhausts all iterations without a finish signal, it constructs:
```rust
return Err(loop_error(LoopErrorKind::MaxIterations {
    iter: guard_config.max_iterations,
    max: guard_config.max_iterations,
}));
```
This sets `iter` to `guard_config.max_iterations` (the configured limit) instead of the actual number of iterations attempted. Since the `for` loop on line 132 iterates `0..guard_config.max_iterations`, when all are exhausted the actual iteration count equals `max_iterations`, so this is technically a cosmetic bug for the error message. However, the `iter` field is semantically supposed to represent "iterations attempted" while `max` is the limit. The immediate output happens to be the same, but if the loop body were ever changed to increment differently (e.g., retry logic within a turn), this would silently report wrong values. Furthermore, the `Display` impl (line 64) says "max iterations ({max}) reached without completion (attempted {iter})" which is misleading even in the current code -- it says "attempted 20" when it should convey that 20 iterations were ALL exhausted, but `max` should be on the limit side.

**Fix:**
```rust
return Err(loop_error(LoopErrorKind::MaxIterations {
    iter: turn + 1,  // or: guard_config.max_iterations (current turn count after loop exit)
    max: guard_config.max_iterations,
}));
```
The simplest fix is to compute the iteration count from the loop variable: `iter: guard_config.max_iterations` is correct in this specific construct (because `turn` starts at 0 and goes to `max-1`, so after the loop the count is `max`). But for clarity, use `iter: guard_config.max_iterations` which is what the code already does. The real issue is that `iter` and `max` can never diverge. The better fix is to capture the final turn value before the `return`:
```rust
let attempted = turn + 1; // turn is 0-indexed, +1 for human-readable count
return Err(loop_error(LoopErrorKind::MaxIterations {
    iter: attempted,
    max: guard_config.max_iterations,
}));
```
But since `turn` is not in scope outside the `for` loop (it ends at `max_iterations - 1` only because the loop completes), the exact value after loop exhaustion equals `max_iterations`. The code is technically correct; marking as CR-02 for the misleading code structure that would break silently if the loop construct changes.

## Warnings

### WR-01: Unbounded LLM message history accumulation (context window exhaustion)

**File:** `crates/jadepaw-agent/src/loop.rs:240-254, 256-263`
**Issue:** The `messages` vector grows unboundedly across turns. Each `Act` branch appends both an observation result and the assistant's full response to the message history. Each `ContinueThinking` branch also appends the assistant's full response. At 20 iterations (default `max_iterations`), the message history may contain up to 21 system+user messages plus 20 observation messages plus 20 assistant responses, easily exceeding LLM context windows. While context window limits also exist at the LLM API level and would produce an error, this wastes tokens on earlier turns that may no longer be relevant. A rolling window or summarization strategy should be documented if deferred to a future phase.

**Fix:** Document the known limitation and add a TODO referencing a future message windowing strategy. At minimum, cap the history size or implement sliding window truncation.

### WR-02: Tool observation result has no size cap for LLM context

**File:** `crates/jadepaw-core/src/tool.rs:63-91` and `crates/jadepaw-agent/src/loop.rs:227-246`
**Issue:** `ToolResult::to_observation_string()` returns the full result data with no size limit. The comment at line 60-62 says "the caller should truncate the output to a reasonable size (e.g., 50KB) before appending to the LLM message history" but `react_loop` at line 229-243 does not perform any truncation before appending the observation to messages. A tool returning a large result (e.g., a file read of a 2MB file, or a large HTTP response) would be fed directly into the LLM context, wasting tokens and potentially exceeding context window limits.

**Fix:** Add truncation in `react_loop` before appending the observation to the message history. For example:
```rust
let result_str = tool_result.to_observation_string();
let result_str = if result_str.len() > 50_000 {
    format!("{}...\n[TRUNCATED: {} bytes total]", &result_str[..50_000], result_str.len())
} else {
    result_str
};
```

### WR-03: `tool_registry::call_tool()` performs domain capability check for wrong layer -- domain checks belong in `HttpRequestTool`, not tool registry

**File:** `crates/jadepaw-agent/src/tool_registry.rs:104-163`
**Issue:** The `call_tool()` method checks `tool_id` via `can_call_tool()`, which only checks the `can_exec_tools` whitelist. It does not (and structurally cannot) check `can_access_domain()` because domain information is buried in the tool args which the registry doesn't parse. This is a structural gap: the `ToolRegistry` provides a centralized capability gate but can only check tool-level (not operation-level) capabilities. The `HttpRequestTool` then has no access to the session's capability state to perform its own domain check. CR-01 details the concrete bypass in `HttpRequestTool`; this warning flags the structural tension between centralized capability enforcement in the registry and the need for operation-level (domain-specific) checks that can only happen within the tool impl.

**Fix:** Either: (1) Pass `&SessionState` (or its capabilities subset) to `Tool::call()` alongside the args, or (2) perform domain checks as part of `ToolRegistry::call_tool()` by requiring tools to expose a pre-call validation hook that receives session capabilities. Option (1) is simpler and aligns with the existing `session_id` parameter on `Tool::call()`.

### WR-04: `HttpRequestTool` performs SSRF IP check before domain capability validation (defense ordering)

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:252-256`
**Issue:** The SSRF IP-layer check (`resolve_and_check_ssrf`) runs at line 253 before any domain capability check. If the domain capability check is restored (per CR-01), it must come before DNS resolution and SSRF IP check to match the defense-in-depth ordering documented in the module header (defense layer 2: domain whitelist; defense layer 3: IP-layer SSRF). Currently, DNS resolution happens first (wasting resources and leaking timing info). If the capability check is added after the SSRF check, the ordering would be wrong.

**Fix:** After adding the domain capability check per CR-01, place it before `resolve_and_check_ssrf()` to match the documented defense ordering and avoid unnecessary DNS lookups for domains the session isn't authorized to access.

### WR-05: `llm.rs` `parse_next_action` regex-less parsing misses `ACTION:` inside nested parentheses

**File:** `crates/jadepaw-agent/src/llm.rs:239-262`
**Issue:** The tool parsing logic at line 240 finds the first `(` character and then uses `rfind(')')` at line 243 to find the LAST `)` character. This means if the action string contains nested parentheses (e.g., `ACTION: tool_name(key=func(arg))`), the parsed args would include everything between the first `(` and the last `)`, which in this case would be `key=func(arg` - matching correctly here. However, if there is text after the last `)` (e.g., `ACTION: tool_name(x=1)` extra text`), the parser would capture the entire trailing segment up to the last `)` and silently include ` extra text` in the args. This is a liberal parse that could produce malformed args or unexpected content in the tool arguments.

**Fix:** Use a parenthesis-depth-aware parser or a more conservative parse that finds the matching `)` for the first `(` rather than using `rfind(')')`. A simple approach: scan for balanced parentheses starting from the opening `(`.

## Info

### IN-01: `#[allow(dead_code)]` on `FileReadTool` and `FileWriteTool` structs

**File:** `crates/jadepaw-wasm/src/tool_impls/file_tool.rs:30, 149`
**Issue:** Both `FileReadTool` (line 30) and `FileWriteTool` (line 149) carry `#[allow(dead_code)]` annotations, indicating the compiler warns about these types being unused. While these are designed to be constructed dynamically via `ToolRegistry::register()`, the lint suppression masks the fact that these types are not constructed anywhere in the production code path. If they are indeed used only through dynamic registration, the annotation should have a comment explaining why.

**Fix:** Remove the `#[allow(dead_code)]` annotations and add explicit construction calls (e.g., in the agent startup path), or document with a comment that these are constructed only via `ToolRegistry::register()` at runtime and the annotation prevents a false-positive compiler warning.

### IN-02: `tool_registry.rs` `use jadepaw_core::SessionId` is `#[cfg(test)]` gated but imported at module level

**File:** `crates/jadepaw-agent/src/tool_registry.rs:28-29`
**Issue:** `SessionId` is imported with `#[cfg(test)]` at the module level, but the `Tool` trait's `call()` method (used in production code) has `session_id: SessionId` as a parameter. The test-only import works because `SessionId` is transitively available through `jadepaw_core::Tool`, but having an explicit `#[cfg(test)]` import at the module level is misleading -- it suggests the type isn't used in production when it actually is (via `Tool::call`). The `use jadepaw_core::SessionId` line should either be removed (since it's only needed in tests and already available via the trait) or kept unconditionally to document the dependency.

**Fix:** Either remove the `use jadepaw_core::SessionId;` line entirely (the test module can import it separately), or make it unconditional with a comment noting it's used by the `Tool` trait's `call()` signature.

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_