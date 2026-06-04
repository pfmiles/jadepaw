---
phase: 04-tool-system
reviewed: 2026-06-04T22:00:00Z
depth: standard
files_reviewed: 22
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
  - Cargo.lock
findings:
  critical: 1
  warning: 1
  info: 2
  total: 4
status: issues_found
---

# Phase 04: Code Review Report (Re-review After Fixes)

**Reviewed:** 2026-06-04T22:00:00Z
**Depth:** standard
**Files Reviewed:** 22
**Status:** issues_found

## Summary

This is a re-review of Phase 04 (tool-system) after all six prior fix-iteration findings were applied. The review covers 22 files across `jadepaw-core`, `jadepaw-agent`, and `jadepaw-wasm`.

**All six prior fix-report findings are verified as correctly applied:**

1. CR-01 (`checked_add` pattern in bounds check): Refactored closure in `http_request_host_fn` returns `Option<usize>` using `checked_add`. Duplicate `saturating_add` at call sites eliminated. **Confirmed fixed.**
2. WR-01 (SSE termination event for MaxIterations): `react_loop` now sends `ReActStep::Error` before returning the MaxIterations error (line 287). **Confirmed fixed.**
3. WR-02 (dual tracking variables in HttpRequestTool): Single `truncated` flag replaces `total`/`buf.len()` pair. Inline `buf.len() + bytes.len()` check used. **Confirmed fixed.**
4. WR-03 (`as_millis() as u64` truncation in guard.rs): Replaced with `u64::try_from(...).unwrap_or(u64::MAX)`. **Confirmed fixed.**
5. WR-04 (unused `session_id` field in FileReadTool/FileWriteTool): Fields and `#[allow(dead_code)]` removed. Constructor signatures simplified. **Confirmed fixed.**
6. WR-05 (`fa_pos` variable naming in llm.rs fallback branch): Renamed to `final_answer_pos`. **Confirmed fixed.**

**Prior review findings from the most recent 04-REVIEW.md:**

- WR-01 (IPv6 DNS resolution in `resolve_and_check_ssrf_addr`): The unstaged diff in `network.rs` shows the fix has been applied -- `host.contains(':')` now wraps bare IPv6 addresses in brackets before constructing the socket address string. **Confirmed fixed** (unstaged, not yet committed).
- IN-01 (MaxIterations turn field): The `turn` field at line 293 of `loop.rs` still uses `guard_config.max_iterations` (one past the last attempted turn). **Not fixed.**
- IN-02 (saturating_add comment in filesystem.rs): `filesystem.rs` is not in the review file list. Out of scope for re-assessment.
- IN-03 (ToolRegistry domain capability check skip): The `if let` guard at line 156 of `tool_registry.rs` still silently skips the domain check when `url` is missing or not a string. **Not fixed.**

**New finding:** A critical bug in `extract_host_from_url` where an `@` character appearing in the URL path, query string, or fragment causes incorrect host extraction. This affects all three call sites (domain capability check in `tool_registry.rs`, URL validation in `http_tool.rs`, and domain extraction in `network.rs`).

---

## Critical Issues

### CR-01: `extract_host_from_url` incorrectly strips after `@` in URL path/query/fragment, corrupting host extraction

**File:** `crates/jadepaw-core/src/tool.rs:47`
**Also affects:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:172`, `crates/jadepaw-agent/src/tool_registry.rs:156`, `crates/jadepaw-wasm/src/host/network.rs:98,254`

**Issue:** The `extract_host_from_url` function applies userinfo stripping via `find('@')` on the entire URL string AFTER scheme stripping but BEFORE path/query/fragment stripping. This means an `@` character appearing in a URL's path segment, query string, or fragment causes the function to incorrectly treat everything before the `@` as userinfo and return only the text after the `@` as the "host."

For example:
- `https://example.com/path?q=user@host` -- returns `"host"` instead of `"example.com"`
- `https://example.com/path#user@host` -- returns `"host"` instead of `"example.com"`
- `https://example.com/path/foo@bar` -- returns `"bar"` instead of `"example.com"`

This affects all three call sites:
1. **Domain capability check** (`tool_registry.rs:156`): A legitimate URL with `@` in query/path is incorrectly blocked because the extracted host does not match any whitelist entry. The domain check uses the wrong host, causing a false-positive rejection.
2. **URL validation in HttpRequestTool** (`http_tool.rs:172`): The SSRF IP check resolves the wrong host, potentially blocking legitimate requests.
3. **Host function path** (`network.rs:98`): Same incorrect domain extraction for the capability check.
4. **Userinfo stripping in http_request_host_fn** (`network.rs:254`): The `after_scheme.find('@')` call has the same flaw -- it picks up `@` in query/path and corrupts the actual request URL sent to reqwest. A URL like `https://example.com/api?token=abc@xyz` becomes `https://xyz` (completely wrong).

**Root cause:** The code uses `after_scheme.find('@')` to locate the userinfo-host delimiter, but `@` can legally appear in path segments, query strings, and fragment identifiers per RFC 3986. The userinfo delimiter is only the LAST `@` before the first `/`, `?`, or `#`.

**Fix:** Restrict userinfo stripping to the authority portion of the URL (between `://` and the first `/`, `?`, or `#`). Use `rfind('@')` limited to the authority segment:

In `extract_host_from_url` (`crates/jadepaw-core/src/tool.rs`):
```rust
// Strip scheme
let after_scheme = if let Some(idx) = url.find("://") {
    &url[idx + 3..]
} else {
    url
};

// Find the end of the authority component (first /, ?, or #)
let authority_end = after_scheme
    .find('/')
    .or_else(|| after_scheme.find('?'))
    .or_else(|| after_scheme.find('#'))
    .unwrap_or(after_scheme.len());

let authority = &after_scheme[..authority_end];

// Strip userinfo from the authority portion only.
// Use rfind to find the LAST @ in the authority — per RFC 3986, the
// userinfo is everything before the last @, and the host follows.
let after_userinfo = if let Some(idx) = authority.rfind('@') {
    // CR-01: Reconstruct the full remainder: host from authority +
    // the path/query/fragment portion that was excluded from the search.
    &after_scheme[idx + 1..]  // this includes the path/query/fragment
} else {
    after_scheme
};

// Strip path, query, fragment (unchanged)
let host_and_port = if let Some(idx) = after_userinfo.find('/') {
    &after_userinfo[..idx]
} else if let Some(idx) = after_userinfo.find('?') {
    &after_userinfo[..idx]
} else if let Some(idx) = after_userinfo.find('#') {
    &after_userinfo[..idx]
} else {
    after_userinfo
};

// ... rest unchanged (port/IPv6 stripping)
```

In `http_request_host_fn` (`network.rs:252-261`), apply the same authority-bounded check:
```rust
let request_url = if let Some(scheme_end) = url.find("://") {
    let after_scheme = &url[scheme_end + 3..];
    let authority_end = after_scheme
        .find('/')
        .or_else(|| after_scheme.find('?'))
        .or_else(|| after_scheme.find('#'))
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..authority_end];
    if let Some(at_pos) = authority.rfind('@') {
        format!("{}://{}", &url[..scheme_end], &after_scheme[at_pos + 1..])
    } else {
        url.to_string()
    }
} else {
    url.to_string()
};
```

---

## Warnings

### WR-01: `react_loop` MaxIterations SSE error event reports `turn: max_iterations` rather than `turn: max_iterations - 1` (IN-01 from prior review, not yet fixed)

**File:** `crates/jadepaw-agent/src/loop.rs:293`
**Issue:** The ReAct loop uses `for turn in 0..guard_config.max_iterations`, so the last attempted turn index is `max_iterations - 1`. The error event sent before returning MaxIterations sets `turn: guard_config.max_iterations` (one past the last turn). This is minimally misleading -- the error message text correctly describes the condition, and the `turn` field on the SSE event is informational. But it is technically inaccurate for consumers that rely on the turn number for debugging or metrics.

**Fix:** Change line 293 from `turn: guard_config.max_iterations` to `turn: guard_config.max_iterations.saturating_sub(1)` to reflect the actual last attempted turn:
```rust
turn: guard_config.max_iterations.saturating_sub(1),
```

---

## Info

### IN-01: `ToolRegistry::call_tool()` domain capability check for `http_request` silently skips when `url` key is missing or not a string (IN-03 from prior review, not yet fixed)

**File:** `crates/jadepaw-agent/src/tool_registry.rs:155-169`
**Issue:** The `if let` guard at line 156 chains `args.get("url").and_then(|v| v.as_str()).map(extract_host_from_url)`. If the `"url"` key is missing or is not a string, the domain capability check is silently skipped (the `if let` does not execute). The tool is then dispatched to `HttpRequestTool::call()`, where the missing URL is caught at line 247-256 with an `INVALID_ARGS` error. This is functionally correct but violates defense-in-depth by skipping the capability enforcement step entirely when the URL argument is malformed.

**Fix:** The fix from the prior review remains applicable. Extract the host from the raw argument string rather than relying on `as_str()`:
```rust
if name == HTTP_REQUEST_TOOL_NAME {
    if let Some(url_str) = args.get("url").and_then(|v| v.as_str()) {
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
    // If url is missing/not a string, let the tool impl reject it.
}
```

### IN-02: `http_request_host_fn` logs original URL (with userinfo) on request failure, undoing WR-04 credential stripping for error-log paths

**File:** `crates/jadepaw-wasm/src/host/network.rs:306`
**Issue:** Line 306 uses the original `url` variable (which may contain `user:password@` credentials) in the warning log message: `warn!(..., "http_request: request failed for '{}': {}", url, e)`. The request itself was sent to `request_url` (credentials stripped per WR-04 at line 252-255), but the error log still emits the original URL with credentials. If logs are persisted or shipped to an observability platform, this constitutes an information disclosure.

**Fix:** Use `request_url` instead of `url` in the error log:
```rust
warn!(%session_id, "http_request: request failed for '{}': {}", request_url.as_str(), e);
```

---

_Reviewed: 2026-06-04T22:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_