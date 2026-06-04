---
phase: 04-tool-system
reviewed: 2026-06-04T23:30:00Z
depth: standard
files_reviewed: 21
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
  - crates/jadepaw-wasm/src/host/mod.rs
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/mod.rs
findings:
  critical: 0
  warning: 3
  info: 2
  total: 5
status: issues_found
---

# Phase 04: Code Review Report (Re-review After CR-01 and WR-01 Fixes)

**Reviewed:** 2026-06-04T23:30:00Z
**Depth:** standard
**Files Reviewed:** 21
**Status:** issues_found

## Summary

This is a re-review of Phase 04 (tool-system) triggered after two targeted fix commits (b2256be for CR-01, 3832048 for WR-01). The review covers 21 source files across `jadepaw-core`, `jadepaw-agent`, and `jadepaw-wasm`.

**Prior open findings status (4 items):**

1. **CR-01 (extract_host_from_url -- @ in path/query/fragment corrupting host extraction):** **CONFIRMED FIXED.** Commit b2256be correctly applies authority-bounded `@` search in both `jadepaw-core/src/tool.rs` (lines 43-65) and `jadepaw-wasm/src/host/network.rs` (lines 258-273). New tests in `tool.rs` cover `@` in query (line 329), fragment (line 338), path (line 347), and combined userinfo + path-`@` (line 356). Tests all verify `@` in non-authority segments is not stripped.

2. **WR-01 (MaxIterations SSE error reports `turn: max_iterations` one past last turn):** **CONFIRMED FIXED.** Commit 3832048 changes `loop.rs:293` to `turn: guard_config.max_iterations.saturating_sub(1)`. For `max_iterations=20`, the loop body executes for turns 0..19 inclusive, then the `for` loop exits. The `saturating_sub(1)` correctly reports turn 19 as the last attempted turn.

3. **IN-01 (ToolRegistry domain capability check silently skips when `url` missing/not string):** **NOT FIXED.** The `if let` guard at `tool_registry.rs:156` still silently skips the domain check when `url` is absent or not a JSON string. Reclassified as WR-02 below since defense-in-depth gaps at security boundaries warrant warning status.

4. **IN-02 (http_request_host_fn logs original URL with userinfo on failure):** **NOT FIXED.** Line 318 of `network.rs` still passes `url` (original, possibly containing `user:password@`) to the warn log, rather than `request_url` (credentials stripped at lines 258-273). Reclassified as WR-03 below since this is an information disclosure path if logs are persisted.

**New finding:** The unstaged diff in `network.rs` changes `resolve_and_check_ssrf_addr` to wrap bare IPv6 addresses in brackets (`[::1]:0` instead of `::1:0`), but this fix is uncommitted. Additionally, `HttpRequestTool::name()` returns a hardcoded string literal independent of the `HTTP_REQUEST_TOOL_NAME` constant, creating a divergence risk.

No critical findings remain. The two prior critical-level issues (CR-01 URL parsing, and the Phase 2 `checked_add`/`saturating_add` pattern) are both fully resolved.

---

## Warnings

### WR-01: IPv6 DNS resolution fix in `resolve_and_check_ssrf_addr` is uncommitted (unstaged)

**File:** `crates/jadepaw-wasm/src/host/network.rs:367`
**Issue:** The unstaged diff changes `resolve_and_check_ssrf_addr` to wrap bare IPv6 addresses in brackets before constructing the socket address for DNS lookup. The current committed code constructs `format!("{}:0", host)`, which for a bare IPv6 address like `::1` produces `"::1:0"` -- an ambiguous string that `tokio::net::lookup_host` may interpret incorrectly (e.g., as `::1` scope with port 0, or as an invalid IPv6 address with trailing `:0`). The fix wraps IPv6 addresses in brackets: `format!("[{}]:0", host)` produces `"[::1]:0"` which is unambiguous per RFC 3986.

The fix is already written and tested (appears to work correctly), but it resides only in the working tree and has not been committed. Until committed, any fresh checkout or CI run will use the old broken code.

**Fix:** Commit the unstaged change or stage it for the next commit. The staged/committed diff should be:
```rust
let ported = if host.contains(':') {
    format!("[{}]:0", host)
} else {
    format!("{}:0", host)
};
```

---

### WR-02: `ToolRegistry::call_tool()` domain capability check silently skips when `url` argument is missing or not a string (IN-01 elevated to warning)

**File:** `crates/jadepaw-agent/src/tool_registry.rs:155-169`
**Issue:** The `if let` guard chains `args.get("url").and_then(|v| v.as_str()).map(extract_host_from_url)`. When the `"url"` key is absent or its value is not a JSON string (e.g., an object, number, or null), the entire `if let` body is skipped and the domain capability check is bypassed. The tool is then dispatched to `HttpRequestTool::call()`, which catches the missing URL at lines 247-256 with `INVALID_ARGS` error.

While the tool impl correctly rejects the malformed request, the capability enforcement step is skipped entirely. In a defense-in-depth model, the capability gate should fail closed (deny) rather than open (skip). If the tool impl were to ever relax its input validation (e.g., adding a default URL), the bypass would become exploitable.

Additionally, the `can_access_domain` call on a host extracted from a malformed URL value could produce unexpected results. For example, `args.get("url")` returning a JSON number like `42` would have `as_str()` return `None`, so the check is skipped. But if the tool impl parsed it as a URL, the capability check never ran.

**Fix:** Separate the URL extraction from the domain check logic so that the check is only skipped when the URL is truly absent, and an invalidly-typed URL triggers a deny:
```rust
if name == HTTP_REQUEST_TOOL_NAME {
    let url_val = args.get("url");
    if let Some(url_str) = url_val.and_then(|v| v.as_str()) {
        let host = extract_host_from_url(url_str);
        if !host.is_empty() && !state.can_access_domain(host) {
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
    // When url is missing or not a string, let the tool impl reject it with INVALID_ARGS.
    // The capability check is deferred (not skipped) since there is no domain to check.
}
```

---

### WR-03: `http_request_host_fn` logs original URL (with userinfo credentials) on request failure

**File:** `crates/jadepaw-wasm/src/host/network.rs:318`
**Issue:** Line 318 passes `url` (the raw guest-provided URL, which may contain `user:password@` credentials) to the `warn!` macro:
```rust
warn!(%session_id, "http_request: request failed for '{}': {}", url, e);
```
The request itself is sent to `request_url` (credentials stripped at lines 258-273, the CR-01 fix), so the outbound connection is safe. However, the error log still includes the raw URL. If logs are persisted, aggregated to an observability platform, or inspected by operators, this constitutes information disclosure of credentials embedded by guest Wasm code in the URL.

Note: The same pattern at line 108 also logs `url` in the capability-denied warning:
```rust
warn!(%session_id, "http_request: CapabilityDenied for domain '{}' (URL: {})", domain, url);
```
This is a less severe case (the request never goes out), but still unnecessary credential exposure.

**Fix:** Use `request_url` (credentials stripped) or `domain` (host only) in all log messages:
- Line 108: Use `request_url` or `domain` instead of `url`
- Line 318: Use `request_url.as_str()` instead of `url`

```rust
// Line 108:
warn!(%session_id, "http_request: CapabilityDenied for domain '{}'", domain);

// Line 318:
warn!(%session_id, "http_request: request failed for '{}': {}", request_url, e);
```

---

## Info

### IN-01: `HttpRequestTool::name()` hardcoded string diverges from `HTTP_REQUEST_TOOL_NAME` constant

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:49,191`
**Issue:** `HTTP_REQUEST_TOOL_NAME` is defined as a `pub const` at line 49 (`"http_request"`) and used by `ToolRegistry::call_tool()` (line 155 of `tool_registry.rs`) to gate the domain capability check. However, `HttpRequestTool::name()` at line 191 returns the hardcoded literal `"http_request"` rather than referencing the constant. If someone later changes `HTTP_REQUEST_TOOL_NAME` to a different value (e.g., `"http_fetch"`), the `name()` method would still return `"http_request"`, causing a silent mismatch: the domain capability check in `ToolRegistry` would stop matching, and the check would be silently bypassed for all http_request calls.

**Fix:** Use the constant in the `name()` method:
```rust
fn name(&self) -> &str {
    HTTP_REQUEST_TOOL_NAME
}
```
This ensures the tool's name and the capability-check gate always stay in sync.

---

### IN-02: `http_tool.rs` toctou DNS rebinding risk is documented only in the tool API path, not in the host function path

**File:** `crates/jadepaw-wasm/src/host/network.rs:206`
**Issue:** `http_request_host_fn` performs SSRF IP validation via `resolve_and_check_ssrf_addr` (line 206) but then passes `request_url` to reqwest which performs its own independent DNS resolution. This creates the same TOCTOU DNS rebinding window that `http_tool.rs` documents extensively (module-level doc at lines 17-29 and the `let _ = &addrs` annotation at line 279). However, `http_request_host_fn` does not document this accepted risk anywhere -- the `_addrs` prefix on line 206 silently drops the validated result without explanation. A future developer unfamiliar with the DNS rebinding concern might "fix" the unused variable warning by removing the SSRF check entirely, not realizing it serves as defense-in-depth.

**Fix:** Add a comment above line 206 documenting why the result is discarded:
```rust
// NOTE: resolve_and_check_ssrf_addr validates that the domain resolves to
// public IPs, but the validated addresses are not pinned to the subsequent
// reqwest request. reqwest performs its own internal DNS resolution, creating
// a TOCTOU window for DNS rebinding. This is accepted risk for MVP -- the
// domain whitelist (can_access_domain, checked above) is the primary defense.
// See http_tool.rs module-level docs for full rationale.
let _addrs = match resolve_and_check_ssrf_addr(&domain).await {
```

---

_Reviewed: 2026-06-04T23:30:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_