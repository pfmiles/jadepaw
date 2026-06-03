---
phase: 04-tool-system
fixed_at: 2026-06-03T00:00:00Z
review_path: .planning/phases/04-tool-system/04-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 9
skipped: 0
status: all_fixed
---

# Phase 04: Code Review Fix Report

**Fixed at:** 2026-06-03T00:00:00Z
**Source review:** .planning/phases/04-tool-system/04-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 9 (3 Critical + 6 Warning)
- Fixed: 9
- Skipped: 0

## Fixed Issues

### CR-01: Domain capability check (can_access_domain) completely bypassed in HttpRequestTool::call()

**Files modified:** `crates/jadepaw-agent/src/tool_registry.rs`
**Commit:** `7aa176c`
**Applied fix:** Added domain capability enforcement in `ToolRegistry::call_tool()` where `SessionState` is available. For `http_request` tool invocations, extracts the hostname from the tool args and calls `state.can_access_domain()` before dispatch. Also removed the stale `#[cfg(test)]` gate on `SessionId` import (addressing IN-04).

### CR-02: http_request_host_fn reads unlimited response body into memory with no cap enforcement

**Files modified:** `crates/jadepaw-wasm/src/host/network.rs`
**Commit:** `def960c`
**Applied fix:** Replaced `response.bytes().await` (which reads the entire body into memory) with `drop(response)`. Since this host function only returns `status_code` and never uses the body data, dropping the response avoids the DoS vector while reqwest's connection pool handles cleanup.

### CR-03: HttpRequestTool::call() reads full response body before truncation

**Files modified:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs`, `crates/jadepaw-wasm/Cargo.toml`
**Commit:** `a883950`
**Applied fix:** Replaced `response.text().await` (which reads the entire body into a String) with a bounded chunked read loop using `response.chunk()`. The loop accumulates data up to `MAX_RESPONSE_BODY_SIZE` (1MB), then drains remaining chunks to free the connection without allocating more memory. Added `bytes` crate as an explicit dependency.

### WR-01: SSRF IP check races with actual request -- resolved IPs discarded

**Files modified:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs`
**Commit:** `ff71bdb`
**Applied fix:** Renamed `_addrs` to `addrs` (removing the dead-code indicator) and added explicit documentation explaining why the resolved IPs cannot be pinned to the reqwest request without per-request client construction. The DNS rebinding TOCTOU window remains an accepted risk for MVP.

### WR-02: HttpRequestTool sets user-supplied headers without validation

**Files modified:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs`
**Commit:** `82f7803`
**Applied fix:** Added a `FORBIDDEN_REQUEST_HEADERS` blocklist (`host`, `content-length`, `transfer-encoding`, `proxy-authorization`, `connection`, `expect`) and CR/LF injection guard for user-supplied headers. Headers matching the blocklist or containing `\r`/`\n` are silently skipped.

### WR-03: WallClockTimeout elapsed_ms reports the timeout limit, not the actual elapsed time

**Files modified:** `crates/jadepaw-agent/src/guard.rs`
**Commit:** `a9a601c`
**Applied fix:** Added a `start = tokio::time::Instant::now()` before the `tokio::select!` and computes `elapsed_ms` from `start.elapsed()` when the timeout branch fires. The configured limit is now correctly reported as `max_ms`, distinct from the actual elapsed time.

### WR-04: Unbounded LLM message history accumulation across turns

**Files modified:** `crates/jadepaw-agent/src/loop.rs`
**Commit:** `ac00209`
**Applied fix:** Added a TODO comment documenting the known limitation and outlining potential mitigation strategies (sliding window, summarization, token counting) for future implementation.

### WR-05: Command injection risk via unsanitized parser output from llm.rs

**Files modified:** `crates/jadepaw-agent/src/llm.rs`
**Commit:** `d8fa94f`
**Applied fix:** Replaced the simple `rfind(')')` approach in `parse_next_action()` with depth-tracking parenthesis matching. The parser now iterates through characters counting `(` and `)` to find the true matching close paren, correctly handling nested parentheses and rejecting malformed input.

### WR-06: Tool trait signature prevents per-operation capability enforcement

**Files modified:** `crates/jadepaw-core/src/tool.rs`
**Commit:** `b871acb`
**Applied fix:** Added documentation to the `Tool` trait explaining that per-operation capability enforcement must happen at the Registry level because `Tool::call()` only receives `SessionId`. Referenced the http_request domain check in `tool_registry.rs` as an implementation example.

---

_Fixed: 2026-06-03T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_