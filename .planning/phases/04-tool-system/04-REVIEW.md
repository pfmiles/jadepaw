---
phase: 04-tool-system
reviewed: 2026-06-04T20:00:00Z
depth: standard
files_reviewed: 21
files_reviewed_list:
  - crates/jadepaw-core/src/tool.rs
  - crates/jadepaw-agent/src/tool_registry.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-core/tests/agent_types.rs
  - crates/jadepaw-core/tests/host_functions.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-wasm/src/tool_impls/mod.rs
  - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  - crates/jadepaw-wasm/Cargo.toml
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/host/mod.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
findings:
  critical: 0
  warning: 1
  info: 3
  total: 4
status: issues_found
---

# Phase 04: Code Review Report (Re-review After Fixes)

**Reviewed:** 2026-06-04T20:00:00Z
**Depth:** standard
**Files Reviewed:** 21
**Status:** issues_found

## Summary

This is a re-review of Phase 04 (tool-system) after all six prior findings (CR-01 resource leak, WR-01 Default impl panic, WR-02 TOCTOU lookup, WR-03 forbidden headers, WR-04 credential stripping) have been fixed. The review covers 21 source files across `jadepaw-core`, `jadepaw-agent`, and `jadepaw-wasm`.

**Prior fixes: all verified as correctly applied:**

- CR-01: `SessionState::http_client` (shared reqwest::Client) initialized in `SessionState::new()`. `http_request_host_fn` reads from `caller.data().http_client.clone()`. Redirect limit and timeout are set during construction. **Confirmed fixed.**
- WR-01: `HttpRequestTool::Default` impl removed. Only the fallible `new()` constructor remains. **Confirmed fixed.**
- WR-02: `ToolRegistry::get_by_name()` returns `Option<(ToolId, Arc<dyn Tool>)>`. `call_tool()` destructures `(tool_id, tool)` from a single lookup. **Confirmed fixed.**
- WR-03: `http_request_host_fn` includes `FORBIDDEN_REQUEST_HEADERS` blocklist and CR/LF injection check (lines 269-298) matching `HttpRequestTool::call()`. **Confirmed fixed.**
- WR-04: `http_request_host_fn` strips userinfo from URL before passing to reqwest (lines 252-261). **Confirmed fixed.**

The SSRF defense-in-depth (scheme validation, domain whitelist, IP-layer check via `is_blocked_ip`, IPv4-mapped IPv6 handling, redirect limiting, body cap, timeout) is consistent across both the Tool API path and the Wasm host function path.

This re-review found **1 warning** (IPv6 DNS resolution in the shared SSRF check) and **3 info** items.

## Warnings

### WR-01: `resolve_and_check_ssrf_addr` produces an invalid socket address string for bare IPv6 addresses, blocking all IPv6 requests

**File:** `crates/jadepaw-wasm/src/host/network.rs:355`
**Issue:** The shared SSRF DNS resolution function uses `format!("{}:0", host)` to construct a socket address string for `tokio::net::lookup_host`. When `host` is an IPv6 address (brackets already stripped by `extract_host_from_url`), this produces a string like `2001:db8::1:0`, which is not a valid `host:port` format for `ToSocketAddrs`.

The Rust standard library's `ToSocketAddrs` for `&str` requires IPv6 addresses to be enclosed in square brackets with the port appended: `[2001:db8::1]:0`. The un-bracketed form `2001:db8::1:0` causes `lookup_host` to fail with a parse error because the last `:` is ambiguous (it could be part of the IPv6 address or the port separator).

This affects both callers of `resolve_and_check_ssrf_addr`:
- `http_request_host_fn` (network.rs line 206) -- any guest Wasm request to an IPv6 URL will fail at the SSRF check
- `HttpRequestTool::call()` (http_tool.rs line 269 via `resolve_and_check_ssrf` wrapper) -- any agent tool call to an IPv6 URL will fail at the SSRF check

The net effect is that **all HTTP requests to raw IPv6 address URLs are blocked**, regardless of whether the IPv6 address is public or the domain is whitelisted. Since most real-world URLs use hostnames (which resolve via DNS without hitting this parsing issue), the practical impact is limited to URLs that embed literal IPv6 addresses.

**Fix:** Wrap the host in brackets when it contains a colon (indicating IPv6):
```rust
pub(crate) async fn resolve_and_check_ssrf_addr(host: &str) -> Result<Vec<std::net::SocketAddr>, SsrfDnsError> {
    let ported = if host.contains(':') {
        format!("[{}]:0", host)
    } else {
        format!("{}:0", host)
    };
    let lookup = tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host(&ported)).await;
    // ... rest unchanged
```

## Info

### IN-01: `react_loop` MaxIterations SSE error event reports `turn: max_iterations` rather than `turn: max_iterations - 1`

**File:** `crates/jadepaw-agent/src/loop.rs:287-294`
**Issue:** The ReAct loop uses `for turn in 0..guard_config.max_iterations`, so the last attempted turn index is `max_iterations - 1`. The error event sent before returning MaxIterations sets `turn: guard_config.max_iterations` (one past the last turn). This is minimally misleading -- the error message text correctly says "max iterations ({max}) reached", so the termination reason is clear. The `turn` field on the SSE event is informational and won't cause functional problems.

**Fix:** Change line 293 from `turn: guard_config.max_iterations` to `turn: guard_config.max_iterations.saturating_sub(1)` to reflect the actual last attempted turn.

### IN-02: `file_read_host_fn` and `file_write_host_fn` use `saturating_add` with a misleading comment referencing `checked_add`

**File:** `crates/jadepaw-wasm/src/host/filesystem.rs:64, 175, 191`
**Issue:** The bounds-checking code in `file_read_host_fn` and `file_write_host_fn` uses `path_start.saturating_add(path_len_usize)` with a code comment that says "WR-01: use checked_add to prevent overflow". The comment is stale -- it dates from the prior review fix which refactored `http_request_host_fn` to use `checked_add` in its `check` closure. The filesystem host functions were not refactored and still use `saturating_add`.

This is functionally safe because `saturating_add` saturates at `usize::MAX`, and the subsequent `> mem_size` check catches any overflow since `usize::MAX > mem_size` for any real memory configuration. But the misleading comment is confusing for readers.

**Fix:** Update the comment to accurately describe the pattern used here, or refactor to use `checked_add` for consistency with `http_request_host_fn`:
```rust
// Bounds-check path pointer (checked_add prevents overflow from hostile guest values)
let path_start = path_ptr as usize;
let path_len_usize = path_len as usize;
let path_end = path_start.checked_add(path_len_usize)
    .filter(|&end| end <= mem_size)
    .unwrap_or_else(|| {
        warn!(%session_id, "file_read: path pointer out of bounds");
        return -1;
    });
```

### IN-03: `ToolRegistry::call_tool()` domain capability check for `http_request` focuses on the `"url"` key but the JSON Schema marks `"url"` as required; hosts extracted from non-present URLs are silently skipped

**File:** `crates/jadepaw-agent/src/tool_registry.rs:155-169`
**Issue:** The domain capability check at line 156 extracts the URL host from `args.get("url").and_then(|v| v.as_str()).map(extract_host_from_url)`. If the `"url"` key is missing or is not a string, this whole chain evaluates to `None` and the `if let` guard at line 156 does not execute -- the domain check is silently skipped. The tool is then dispatched to `HttpRequestTool::call()`, where the missing URL is caught at line 247-256 with an `INVALID_ARGS` error.

This is functionally correct (the tool call ultimately fails with a clear error), but the silent skip means a valid domain capability is not enforced when the URL argument is malformed. In practice, this is not exploitable because the tool itself rejects the call, but the defense-in-depth principle suggests the capability gate should err on the side of blocking.

**Fix:** Consider checking the domain BEFORE validate_url, extracting the host directly from the raw argument string rather than through `as_str()`:
```rust
if name == HTTP_REQUEST_TOOL_NAME {
    if let Some(url_str) = args.get("url").and_then(|v| v.as_str()) {
        let host = extract_host_from_url(url_str);
        if !host.is_empty() && !state.can_access_domain(host) {
            return ToolResult::from_error("CAPABILITY_DENIED", ...);
        }
    }
    // If url is missing/not a string, let the tool impl reject it -- we
    // don't fabricate a domain restriction error for a malformed request.
}
```

---

_Reviewed: 2026-06-04T20:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_