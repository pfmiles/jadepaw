---
phase: 04-tool-system
reviewed: 2026-06-03T12:00:00Z
depth: standard
files_reviewed: 22
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
  - Cargo.lock
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
findings:
  critical: 2
  warning: 2
  info: 1
  total: 5
status: issues_found
---

# Phase 04: Code Review Report (Re-review)

**Reviewed:** 2026-06-03T12:00:00Z
**Depth:** standard
**Files Reviewed:** 22
**Status:** issues_found

## Summary

This is the second adversarial re-review of Phase 04 (tool-system). All three warnings from the previous review (WR-01 shared constant for tool name, WR-02 deduplicated `extract_host_from_url`, WR-03 DNS resolution TODO) have been correctly fixed in commits 6403a43, a7bcb0b, and 4a1972e. The three info-level items (IN-01 dead session_id fields, IN-02 redundant lookup, IN-03 expect in library code) intentionally remain.

However, this re-review uncovered two new issues not caught in the prior review:

1. **CR-01 (Critical)**: `extract_host_from_url` is vulnerable to URL credential-based domain whitelist bypass. URLs containing a username that matches a whitelisted domain (e.g., `http://api.example.com:password@evil.com/path`) cause the function to return the username as the "host", allowing the request to pass the `can_access_domain` check while reqwest connects to the real host after the `@`.
2. **CR-02 (Critical)**: The same credential-parsing flaw affects `HttpRequestTool::validate_url`, allowing a request with a fake whitelisted-domain username to pass Host validation and enter the SSRF check path against the wrong hostname.

Two additional warnings cover silent error handling and missing test coverage.

## Critical Issues

### CR-01: Domain whitelist bypass via URL credentials in `extract_host_from_url`

**File:** `crates/jadepaw-core/src/tool.rs:35-60`
**Issue:** The `extract_host_from_url` function strips the port by finding the first `:` after `://`:

```rust
if let Some(idx) = host_and_port.find(':') {
    &host_and_port[..idx]
}
```

This incorrectly handles URLs containing userinfo components (RFC 3986 `user:password@host`). For a URL like `http://api.example.com:mypassword@evil.com/path`:

1. `after_scheme` = `"api.example.com:mypassword@evil.com/path"`
2. `host_and_port` = `"api.example.com:mypassword@evil.com"` (stripped path)
3. `host_and_port.find(':')` finds the `:` in `api.example.com:mypassword`, returning `"api.example.com"`

The function returns `"api.example.com"` (the username) instead of `"evil.com"` (the real host). This is used by two security-critical call sites:

- **`ToolRegistry::call_tool`** (`crates/jadepaw-agent/src/tool_registry.rs:161`): The domain capability check calls `state.can_access_domain(host)` with the **wrong** hostname. If `api.example.com` is whitelisted, the check passes. But reqwest connects to `evil.com`.

- **`http_request_host_fn`** (`crates/jadepaw-wasm/src/host/network.rs:100`): Same function used for the Wasm host function domain check. Same bypass applies.

**Impact:** An attacker whose session has `api.example.com` in the `can_network_to` whitelist can access any arbitrary domain by constructing URLs like `http://api.example.com:anything@target.evil.com/`. The domain whitelist (the primary SSRF defense layer) is completely bypassed. The IP-layer SSRF check in `resolve_and_check_ssrf` provides a secondary defense but does not close this gap for public IPs.

**Fix:** Strip userinfo before host extraction. Add a step between scheme removal and path removal to strip the `user:password@` portion:

```rust
pub fn extract_host_from_url(url: &str) -> &str {
    // Strip scheme
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Strip userinfo (user:password@) — CR-01 fix
    let after_userinfo = if let Some(idx) = after_scheme.find('@') {
        &after_scheme[idx + 1..]
    } else {
        after_scheme
    };

    // Strip path, query, fragment
    let host_and_port = if let Some(idx) = after_userinfo.find('/') {
        &after_userinfo[..idx]
    } else if let Some(idx) = after_userinfo.find('?') {
        &after_userinfo[..idx]
    } else if let Some(idx) = after_userinfo.find('#') {
        &after_userinfo[..idx]
    } else {
        after_userinfo
    };

    // Strip port
    if let Some(idx) = host_and_port.find(':') {
        &host_and_port[..idx]
    } else {
        host_and_port
    }
}
```

Additionally, add a test for credential-bearing URLs and one with `@` in the auth component:

```rust
#[test]
fn extract_host_with_userinfo() {
    // CR-01: userinfo must be stripped before host extraction
    assert_eq!(extract_host_from_url("http://user:pass@example.com/path"), "example.com");
    assert_eq!(extract_host_from_url("http://whitelisted.com:secret@evil.com/api"), "evil.com");
    assert_eq!(extract_host_from_url("https://user@example.com"), "example.com");
}
```

---

### CR-02: `HttpRequestTool::validate_url` uses wrong hostname when URL contains credentials

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:145-175, 254-257`
**Issue:** `HttpRequestTool::validate_url` calls `extract_host_from_url(url_str)` at line 162, which has the same credential bypass as described in CR-01. The extracted hostname is then passed to `resolve_and_check_ssrf(&host)` at line 265. This means:

1. DNS resolution + SSRF IP check is performed against the **username** (e.g., `api.example.com`) instead of the **real host** (e.g., `evil.com`)
2. The SSRF check passes because `api.example.com` resolves to a public IP
3. reqwest connects to `evil.com` without SSRF IP verification

The defense-in-depth SSRF IP layer is rendered ineffective for credential-bearing URLs.

**Impact:** Same as CR-01 — the IP-layer SSRF check protects the wrong hostname and the real hostname receives no SSRF verification. The `is_blocked_ip` check never executes against `evil.com`'s IP addresses.

**Fix:** This is fixed automatically by the CR-01 fix to `extract_host_from_url`, since `validate_url` delegates to that function. No separate code change needed in `HttpRequestTool`, but the test suite should verify that credential-bearing URLs are rejected by the domain capability check at the registry level. Add a test:

```rust
#[test]
fn extract_host_rejects_userinfo_url() {
    // Verify CR-01 fix: userinfo portion is not treated as host
    let host = extract_host_from_url("http://whitelisted.com:pwd@evil.com/path");
    assert_eq!(host, "evil.com");
}
```

---

## Warnings

### WR-01: `extract_host_from_url` does not handle IPv6 bracket notation

**File:** `crates/jadepaw-core/src/tool.rs:54-59`
**Issue:** The port-stripping step finds the first `:`, which breaks on IPv6 addresses enclosed in brackets. For `https://[2001:db8::1]:8080/path`:

1. `after_scheme` = `"[2001:db8::1]:8080/path"`
2. `host_and_port` = `"[2001:db8::1]:8080"` (correct — `/` found before `[` ends)
3. `host_and_port.find(':')` finds the `:` inside `[2001:db8`, returning `"[2001"`

The correct host should be `2001:db8::1`, but `"[2001"` is returned. This causes all IPv6 URLs to fail the domain capability check (default-deny since `"[2001"` won't match any `DomainPattern`).

**Impact:** IPv6 outbound connections are effectively blocked even when the domain is whitelisted. Not a security bypass (errs on denial side), but a functionality regression for environments using IPv6.

**Fix:** Detect bracket notation before stripping the port:

```rust
// Strip port — handle IPv6 bracket notation [::1]:8080
if host_and_port.starts_with('[') {
    if let Some(idx) = host_and_port.find("]:") {
        &host_and_port[1..idx]
    } else if let Some(idx) = host_and_port.find(']') {
        &host_and_port[1..idx]
    } else {
        host_and_port
    }
} else if let Some(idx) = host_and_port.find(':') {
    &host_and_port[..idx]
} else {
    host_and_port
}
```

---

### WR-02: Silently skipping forbidden headers and CR/LF values without logging

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:304-321`
**Issue:** The request header validation loop silently skips forbidden headers (lines 314-316) and headers with CR/LF values (lines 317-319) using `continue`. No warning is logged, making it impossible to detect if an LLM is attempting to send prohibited headers or if there is a header injection attempt.

```rust
for (key, value) in &headers {
    let key_lower = key.to_lowercase();
    if FORBIDDEN_REQUEST_HEADERS.contains(&key_lower.as_str()) {
        continue;  // silently dropped — no logging
    }
    if value.contains('\r') || value.contains('\n') {
        continue;  // silently dropped — no logging
    }
    request = request.header(key.as_str(), value.as_str());
}
```

**Impact:** Security-relevant events (attempted header injection, LLM requesting forbidden headers) are invisible in production logs. Debugging network issues caused by dropped headers is difficult.

**Fix:** Add `tracing::warn` calls:

```rust
if FORBIDDEN_REQUEST_HEADERS.contains(&key_lower.as_str()) {
    tracing::warn!(
        header = %key,
        "http_request: forbidden header '{}' was dropped",
        key
    );
    continue;
}
if value.contains('\r') || value.contains('\n') {
    tracing::warn!(
        header = %key,
        "http_request: header '{}' value contains CR/LF — possible injection attempt, header dropped",
        key
    );
    continue;
}
```

---

## Info

### IN-01: `host_functions.rs` test does not exercise `http_request` trait method

**File:** `crates/jadepaw-core/tests/host_functions.rs:60-89`
**Issue:** The `test_host_functions_trait_result_types` test verifies correct `Result` types for `log_message`, `file_read`, and `file_write`, but does not test `http_request`. The `TestHostFn` struct implements `http_request` (lines 37-48), but the test at lines 60-89 only exercises three of four methods. Any future signature change to `http_request` that breaks the return type would not be caught by this test.

**Fix:** Add an `http_request` invocation to the test:

```rust
let result: Result<(u16, std::collections::HashMap<String, String>, Vec<u8>)> =
    rt.block_on(host.http_request(
        "GET".into(),
        "http://example.com".into(),
        std::collections::HashMap::new(),
        None,
    ));
assert!(result.is_err());
match result.unwrap_err() {
    JadepawError::CapabilityDenied { operation, .. } => {
        assert_eq!(operation, "http_request");
    }
    other => panic!("expected CapabilityDenied, got {other:?}"),
}
```

---

_Reviewed: 2026-06-03T12:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_