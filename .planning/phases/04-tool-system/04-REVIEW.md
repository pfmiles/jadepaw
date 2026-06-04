---
phase: 04-tool-system
reviewed: 2026-06-04T23:30:00Z
updated: 2026-06-04T16:30:00Z
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
  warning: 2
  info: 2
  total: 4
  resolved:
    wr-01: fixed (ba14b49)
    wr-02: wont-fix
    wr-03: wont-fix
status: clean
---

# Phase 04: Code Review Report (Re-review After CR-01 and WR-01 Fixes)

**Reviewed:** 2026-06-04T23:30:00Z
**Depth:** standard
**Files Reviewed:** 21
**Status:** issues_found

## Summary

This is a re-review of Phase 04 (tool-system) triggered after two targeted fix commits (b2256be for CR-01, 3832048 for WR-01). The review covers 21 source files across `jadepaw-core`, `jadepaw-agent`, and `jadepaw-wasm`.

**Resolved findings:**

1. **CR-01 (extract_host_from_url -- @ in path/query/fragment corrupting host extraction):** **FIXED.** Commit b2256be correctly applies authority-bounded `@` search in both `jadepaw-core/src/tool.rs` and `jadepaw-wasm/src/host/network.rs`.
2. **WR-01 (MaxIterations SSE error reports `turn: max_iterations` one past last turn):** **FIXED.** Commit 3832048 changes `loop.rs:293` to `turn: guard_config.max_iterations.saturating_sub(1)`.
3. **WR-01 (IPv6 DNS resolution fix in resolve_and_check_ssrf_addr):** **FIXED.** Commit ba14b49 wraps bare IPv6 addresses in brackets (`[::1]:0`) per RFC 3986.
4. **WR-02 (ToolRegistry domain capability check silently skips):** **WON'T FIX.** Defense-in-depth hardening — the tool impl correctly rejects malformed URLs with `INVALID_ARGS`. Deferred to a future security hardening phase.
5. **WR-03 (http_request_host_fn logs original URL on failure):** **WON'T FIX.** Log sanitization hardening — the outbound request is safe (uses stripped URL). Deferred until logging/observability strategy is defined.

**Remaining open (info only):**

- **IN-01:** `HttpRequestTool::name()` hardcoded literal diverges from `HTTP_REQUEST_TOOL_NAME` constant.
- **IN-02:** TOCTOU DNS rebinding risk is documented in `http_tool.rs` but not in the host function path.

No critical or warning-level findings remain open. All Phase 4 success criteria are verified.

---

## Warnings

### WR-01: IPv6 DNS resolution fix in `resolve_and_check_ssrf_addr` — **FIXED (ba14b49)**

**File:** `crates/jadepaw-wasm/src/host/network.rs:367`
**Resolution:** Committed in ba14b49. Bare IPv6 addresses are now wrapped in brackets (`[::1]:0`) per RFC 3986 before constructing the socket address for DNS lookup.

---

### WR-02: `ToolRegistry::call_tool()` domain capability check silently skips when `url` argument is missing or not a string — **WON'T FIX**

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

### WR-03: `http_request_host_fn` logs original URL (with userinfo credentials) on request failure — **WON'T FIX**

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
_Updated: 2026-06-04T16:30:00Z (WR-01 fixed, WR-02/WR-03 marked won't fix)_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
_Status: clean — all findings resolved or accepted_