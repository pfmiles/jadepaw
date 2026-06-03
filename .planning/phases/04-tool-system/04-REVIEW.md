---
phase: 04-tool-system
reviewed: 2026-06-03T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - Cargo.lock
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
  critical: 3
  warning: 6
  info: 4
  total: 13
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-06-03T00:00:00Z
**Depth:** standard
**Files Reviewed:** 20
**Status:** issues_found

## Summary

Phase 04 introduces the Tool abstraction layer spanning three crates: `jadepaw-core` (Tool trait + ToolResult + ToolDefinition), `jadepaw-agent` (ToolRegistry + ReAct loop tool dispatch), and `jadepaw-wasm` (FileReadTool, FileWriteTool, HttpRequestTool + Wasm host function network support).

The implementation is structurally sound with clear separation between the Tool trait (agent-level) and HostFunctions trait (Wasm-level). The ReAct loop integration correctly dispatches through ToolRegistry with capability gating at the tool level. However, **three critical security gaps** were found:

1. The domain whitelist (`can_network_to`) is not enforced in the `HttpRequestTool::call()` path -- the Tool trait signature cannot access session capabilities
2. Both HTTP implementations (tool path and host function path) read unlimited response bodies into memory before truncation or discard, creating DoS vectors
3. `http_request_host_fn` reads full response bodies but discards them with no actual size cap enforcement

Additionally, six warnings cover SSRF edge cases, error classification mismatches, header injection risks, and defense ordering. Four info items flag dead code, misleading log values, inconsistent truncation, and fragile comments.

## Critical Issues

### CR-01: Domain capability check (can_access_domain) completely bypassed in HttpRequestTool::call()

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:216-377`
**Issue:** The `HttpRequestTool::call()` method validates the URL scheme and extracts the hostname (line 247-250), then performs an SSRF IP-layer check via `resolve_and_check_ssrf()` (lines 253-256), but **never calls `can_access_domain()`** against the session's `can_network_to` capability list. Defense-in-depth Layer 1 (domain whitelist) is entirely skipped. The Wasm host function path (`http_request_host_fn` in `network.rs:103-112`) correctly enforces this check, but the `HttpRequestTool` (called through `ToolRegistry`) bypasses it.

Note: The module doc comment at line 9 of `http_tool.rs` claims "Domain whitelist check (via `SessionState::can_access_domain`)" as defense layer 2, but this check is never actually performed.

**Root cause:** The `Tool::call()` trait signature is `fn call(&self, args: Value, session_id: SessionId) -> ToolResult` -- it only receives a `SessionId`, not `SessionState` with capabilities. The `HttpRequestTool` has no way to access the `can_network_to` whitelist.

**Impact:** If a session is granted `can_call_tool` for `HttpRequestTool` (tool-level allow), it can make HTTP requests to ANY domain, regardless of the `can_network_to` domain whitelist. The SSRF IP check (defense Layer 3) still blocks private/loopback IPs, but domain-based access control (e.g., limiting to `api.example.com`) is completely bypassed.

**Fix:** Option (A) -- extend `Tool::call()` to accept session state, or Option (B) -- move the domain check into `ToolRegistry::call_tool()` where `SessionHandle` is already available. Option (B) avoids changing the trait:

```rust
// In ToolRegistry::call_tool, before step 3 (dispatch to tool):
if name == "http_request" {
    // Extract host from args and check domain capability
    if let Some(host) = extract_host_from_tool_args(&args) {
        let state = session.store().data();
        if !state.can_access_domain(&host) {
            return ToolResult::from_error(
                "CAPABILITY_DENIED",
                &format!(
                    "Domain '{}' is not in the session's network capability whitelist.",
                    host
                ),
                false,
            );
        }
    }
}
```

---

### CR-02: http_request_host_fn reads unlimited response body into memory with no cap enforcement

**File:** `crates/jadepaw-wasm/src/host/network.rs:278-300`

**Issue:** The Wasm host function `http_request_host_fn()` calls `response.bytes().await` (line 281) which reads the **entire** response body into memory regardless of size. `MAX_BODY_SIZE` is defined as 1MB (line 280) but is only used for an informational warning log (lines 291-296) -- the body is never actually truncated or capped. The function then returns only `status_code as i32` (line 300), discarding the body data entirely.

**Impact:** This is a DoS vector: an attacker who tricks a guest module into requesting a URL serving multi-gigabyte content can exhaust the host process memory, taking down all other sessions sharing the wasmtime process. The `MAX_BODY_SIZE` constant and the warning comment ("Truncate if over limit") indicate truncation was intended but never implemented.

**Fix:** Stream the response body with a bounded reader that stops at MAX_BODY_SIZE. At minimum, use `tokio::io::AsyncReadExt::take()` to cap the read:

```rust
use tokio::io::AsyncReadExt;

// Replace response.bytes().await with bounded read:
let mut buf = Vec::new();
let mut body_stream = response.bytes_stream();
use futures::StreamExt;
let mut total: usize = 0;
while let Some(chunk) = body_stream.next().await {
    match chunk {
        Ok(bytes) => {
            if total + bytes.len() > MAX_BODY_SIZE {
                buf.extend_from_slice(&bytes[..MAX_BODY_SIZE - total]);
                total = MAX_BODY_SIZE;
                // Drain remaining chunks to free connection
                while body_stream.next().await.is_some() {}
                break;
            }
            buf.extend_from_slice(&bytes);
            total += bytes.len();
        }
        Err(e) => { warn!(...); return -1; }
    }
}
```

---

### CR-03: HttpRequestTool::call() reads full response body before truncation

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:321-347`

**Issue:** Same fundamental problem as CR-02 but in the `Tool` trait path. `response.text().await` (line 321) reads the **entire** response body into a `String` before any truncation check. The size check (line 336: `body_bytes.len() > MAX_RESPONSE_BODY_SIZE`) only happens AFTER the full body is already in memory. If a server returns 500MB of text, the host process allocates 500MB before truncating the result to 1MB.

**Impact:** DoS risk through the agent dispatch path. Unlike CR-02 which affects the Wasm FFI path, this affects direct Tool invocation through `ToolRegistry::call_tool()`.

**Fix:** Read the body with a bounded buffer. Replace `response.text().await` with `response.chunk().await` in a loop to stream chunks up to `MAX_RESPONSE_BODY_SIZE + 1`, then detect truncation:

```rust
let mut body_buf = Vec::with_capacity(MAX_RESPONSE_BODY_SIZE);
let mut total: usize = 0;
let mut stream = response.chunk();
while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(bytes) => {
            if total + bytes.len() <= MAX_RESPONSE_BODY_SIZE {
                body_buf.extend_from_slice(&bytes);
            }
            total += bytes.len();
        }
        Err(e) => { ... }
    }
}
let truncated = total > MAX_RESPONSE_BODY_SIZE;
let body_str = String::from_utf8_lossy(&body_buf).to_string();
```

---

## Warnings

### WR-01: SSRF IP check races with actual request -- resolved IPs discarded

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:253-256`

**Issue:** `resolve_and_check_ssrf()` resolves the hostname and validates all IPs, but the result is bound as `let _addrs = ...` (line 253, note the underscore prefix) and never used. The actual HTTP request at line 276 goes through `reqwest`, which performs its own independent DNS resolution. This creates a TOCTOU window: between the SSRF check's DNS resolution and `reqwest`'s DNS resolution, a DNS rebinding attack could change the resolved IP from public to private.

The code acknowledges this risk (module doc lines 18-22), but the architectural gap between DNS-check-time and request-send-time means the SSRF check is advisory for non-cached DNS responses.

**Fix:** Pin DNS resolution by passing checked addresses to reqwest:

```rust
let addrs = resolve_and_check_ssrf(&host).await?;
// Pin DNS to checked addresses
let request = self.client.request(method, &url_str);
// Use reqwest::ClientBuilder::resolve() to pin or add checked host IP mapping
```

---

### WR-02: HttpRequestTool sets user-supplied headers without validation

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:259-283`

**Issue:** User-supplied headers from the `headers` JSON field are directly set on the reqwest request via `request.header(key, value)` (line 283). While `reqwest` blocks setting certain restricted headers (e.g., `Host`), the tool should explicitly deny dangerous header names and sanitize header values containing `\r\n` sequences that could enable HTTP request smuggling.

**Fix:** Add a blocklist and value validator:

```rust
const FORBIDDEN_REQUEST_HEADERS: &[&str] = &[
    "host", "content-length", "transfer-encoding",
    "proxy-authorization", "connection",
];
for (key, value) in &headers {
    let key_lower = key.to_lowercase();
    if FORBIDDEN_REQUEST_HEADERS.contains(&key_lower.as_str()) {
        continue;
    }
    if value.contains('\r') || value.contains('\n') {
        continue;
    }
    request = request.header(key.as_str(), value.as_str());
}
```

---

### WR-03: WallClockTimeout elapsed_ms reports the timeout limit, not the actual elapsed time

**File:** `crates/jadepaw-agent/src/guard.rs:106-114`

**Issue:** The wall-clock timeout branch constructs `AgentTerminationReason::WallClockTimeout` with `elapsed_ms: ms` where `ms` is `config.wall_clock_timeout.as_millis() as u64`. This sets `elapsed_ms` to the configured timeout value (e.g., 300,000ms) rather than the actual elapsed time. The termination reason will read "timed out after 300s" but the actual elapsed time could be slightly different (especially under system load).

**Fix:** Record a start `Instant` and compute actual elapsed:

```rust
let start = tokio::time::Instant::now();
tokio::select! {
    result = agent_loop() => { ... }
    _ = tokio::time::sleep(config.wall_clock_timeout) => {
        let elapsed_ms = start.elapsed().as_millis() as u64;
        Err(JadepawError::agent_terminated(
            AgentTerminationReason::WallClockTimeout {
                elapsed_ms,
                max_ms: config.wall_clock_timeout.as_millis() as u64,
            },
        ))
    }
}
```

---

### WR-04: Unbounded LLM message history accumulation across turns

**File:** `crates/jadepaw-agent/src/loop.rs:246-262`

**Issue:** The `messages` vector grows unboundedly. Each `Act` branch appends an observation result + assistant response (lines 246, 254). Each `ContinueThinking` branch appends the assistant response (line 263). At 20 iterations (default `max_iterations`), the history contains up to 20 assistant responses + 20 observation messages + 1 system + 1 user = 42 messages, easily exceeding LLM context windows with meaningful content. This wastes tokens on earlier turns that may no longer be relevant. While the LLM API would also enforce context limits, the agent should manage its own context budget.

**Fix:** Document the limitation and add a TODO for future message windowing. In the short term, consider a sliding window of the last N messages or implement a summarization strategy.

---

### WR-05: Command injection risk via unsanitized parser output from llm.rs

**File:** `crates/jadepaw-agent/src/llm.rs:239-262`

**Issue:** `parse_next_action()` uses `find('(')` at line 240 and `rfind(')')` at line 243 to extract tool arguments from the ACTION directive. If the action string contains unbalanced parentheses or text after the final `)`, the parser silently produces malformed args. For example:
- `ACTION: tool_name(x=1) extra text` would parse args as `"x=1"` (correct -- rfind gives the last `)`)
- Wait: `rfind(')')` would find position 18, giving `x=1`. So that's correct for trailing text. But:
- `ACTION: tool_name(x=func(y=1))` would parse args as `x=func(y=1` (WRONG -- the closing `)` at position 21, not the second one at position 21. Actually `rfind(')')` would find the one at position 21, which is the correct closing paren. Let me re-trace: the string after `ACTION:` prefix is ` tool_name(x=func(y=1))`. `find('(')` finds `(` at position 12 (after "tool_name"). `rfind(')')` from position 13+ finds `)` at position 23 (the last char). `args_and_close[..23]` = `x=func(y=1)`. This is correct for this example.)

The real issue is when `rfind(')')` finds a `)` from a trailing fragment: `ACTION: tool_name(x=1)) extra` would parse args as `x=1)` (includes the first `)` from extra). This is a correctness-for-input-validity issue -- the parser doesn't validate that the parentheses are balanced.

**Fix:** Implement balanced parenthesis matching instead of `rfind(')')`:

```rust
if let Some(paren_pos) = action_str.find('(') {
    let tool = action_str[..paren_pos].trim().to_string();
    let inner = &action_str[paren_pos + 1..];
    // Find the matching closing paren with depth tracking
    let mut depth = 1;
    let mut close_pos = None;
    for (i, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => { depth -= 1; if depth == 0 { close_pos = Some(i); break; } }
            _ => {}
        }
    }
    if let Some(pos) = close_pos {
        let args = inner[..pos].trim().to_string();
        // ...
    }
}
```

---

### WR-06: Tool trait signature prevents per-operation capability enforcement

**File:** `crates/jadepaw-core/src/tool.rs:143-163`

**Issue:** The `Tool::call()` signature receives only `session_id: SessionId`, not `SessionState` or `InstanceCapabilities`. As demonstrated by CR-01, this means tool implementations that need capability information (domain whitelist, path patterns) cannot access it. The `FileReadTool` and `FileWriteTool` work around this by hardcoding `sandbox_root` in their constructor, but domain-based and path-pattern-based capability enforcement are structurally impossible through the `Tool` trait.

**Fix:** Extend the trait to accept session capabilities:

```rust
async fn call(&self, args: serde_json::Value, session_id: SessionId, capabilities: &InstanceCapabilities) -> ToolResult;
```

This allows `HttpRequestTool` to call `capabilities.can_network_to.contains(...)` and `FileReadTool` to validate paths against `capabilities.can_read_files` without hardcoding the sandbox root.

---

## Info

### IN-01: Dead code -- session_id field in FileReadTool and FileWriteTool

**File:** `crates/jadepaw-wasm/src/tool_impls/file_tool.rs:30-31, 149-150`

**Issue:** Both `FileReadTool` and `FileWriteTool` are annotated with `#[allow(dead_code)]` and contain a `session_id: SessionId` field that is stored in the constructor but never read. The `call()` methods accept a separate `_session_id: SessionId` parameter and ignore the struct field. The field provides no audit, logging, or authorization value.

**Fix:** Remove the `session_id` field from both structs and their constructors, or add `tracing::info!` logs referencing it. Remove the `#[allow(dead_code)]` annotations.

---

### IN-02: LlmDirective::Finish branch references specific line number in code comment

**File:** `crates/jadepaw-agent/src/loop.rs:177-181`

**Issue:** The comment at lines 177-181 explains why `LlmDirective::Finish { thought: _ }` discards the thought field, and references "line 159" (the Thought push at the top of the turn). Hard-coded line numbers in comments become stale when code is refactored.

**Fix:** Replace with an invariant description:

```
// thought field intentionally unused — the complete LLM response was
// already captured as a ReActStep::Thought at the beginning of this
// turn iteration, before the parse_next_action() call.
LlmDirective::Finish { thought: _, answer } => {
```

---

### IN-03: http_request_host_fn reads body for cleanup but never truncates

**File:** `crates/jadepaw-wasm/src/host/network.rs:289-300`

**Issue:** The `http_request_host_fn` reads the full response body via `response.bytes().await` (line 281), checks against `MAX_BODY_SIZE` with only a warning (lines 291-296), then returns only `status_code as i32` (line 300). The body is read for "connection cleanup" but the function neither truncates it to the cap nor returns it to the caller. The comment "Truncate if over limit" and the `MAX_BODY_SIZE` constant look like work-in-progress indicators -- the truncation was planned but not completed.

**Fix:** See CR-02 for the full fix. If the body is truly not needed, avoid reading it entirely:

```rust
// Just drop the response to free connection resources without reading body
drop(response);
return status_code as i32;
```

---

### IN-04: `#[cfg(test)]` gated import of SessionId is misleading

**File:** `crates/jadepaw-agent/src/tool_registry.rs:28-29`

**Issue:** `use jadepaw_core::SessionId;` is gated with `#[cfg(test)]` but the `Tool` trait's `call()` method (used in production) has `session_id: SessionId` in its signature. The test-only import works because `SessionId` is transitively available through other imports (`jadepaw_core::Tool`), but the attribute pattern suggests the type isn't used in production when it actually is.

**Fix:** Remove the `#[cfg(test)]` gate from the import and keep it unconditionally, or remove the import entirely and add a direct import in the `mod tests` block.

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_