---
phase: 04-tool-system
reviewed: 2026-06-03T22:10:00Z
depth: standard
files_reviewed: 22
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
  - crates/jadepaw-wasm/src/host/mod.rs
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
  - crates/jadepaw-wasm/src/tool_impls/mod.rs
findings:
  critical: 2
  warning: 3
  info: 2
  total: 7
status: issues_found
---

# Phase 04: Code Review Report (Second Re-review)

**Reviewed:** 2026-06-03T22:10:00Z
**Depth:** standard
**Files Reviewed:** 22
**Status:** issues_found

## Summary

This is the third adversarial review of Phase 04 (tool-system). Previous review findings (CR-01 userinfo bypass, CR-02 validate_url bypass, WR-01 IPv6 bracket handling, WR-02 silent header drops, IN-01 host_functions test gap) have been correctly fixed in commits `ebd9a27`, `3fbb2fa`, `195b83a`. This re-review targets the current state of the code after those fixes.

Two new critical SSRF bypass vectors were discovered that the existing SSRF IP check does not block. Three warnings address logic edge cases in the LLM parser, missing IP range coverage, and structural duplication. Two info items cover test gaps.

## Critical Issues

### CR-01: IPv4-mapped IPv6 address bypasses SSRF IP check

**File:** `crates/jadepaw-wasm/src/host/network.rs:305-322`
**Issue:** The `is_blocked_ip` function checks IPv6 addresses for loopback (`::1`), unique local (`fc00::/7`), link-local (`fe80::/10`), multicast (`ff00::/8`), and unspecified (`::`). However, it does NOT check for IPv4-mapped IPv6 addresses (prefix `::ffff:0:0/96`).

An attacker can construct a URL using raw IPv6 bracket notation `http://[::ffff:127.0.0.1]/admin` or `http://[::ffff:10.0.0.1]/secret`. The extraction flow:

1. `extract_host_from_url("http://[::ffff:127.0.0.1]/admin")` returns `::ffff:127.0.0.1` (WR-01 IPv6 bracket fix handles this correctly)
2. `is_blocked_ip(IpAddr::V6(...))` is called
3. None of the IPv6 check methods match: `is_loopback()` only covers `::1`, not `::ffff:127.0.0.1`; `is_unique_local()` only covers `fc00::/7`; etc.
4. The IP passes through SSRF validation
5. reqwest connects to `http://[::ffff:127.0.0.1]/admin`, which on many systems resolves to the IPv4 loopback `127.0.0.1`

This also applies to IPv4-mapped private addresses like `::ffff:192.168.1.1` and `::ffff:10.0.0.1`.

**Impact:** All internal IPv4 addresses can be reached by wrapping them in IPv4-mapped IPv6 notation. The domain whitelist in the registry is the primary defense, but if the whitelist contains `*` (allow all domains) or the IP corresponds to a whitelisted domain's DNS record (e.g., an internal IP behind a corporate DNS), the SSRF IP-layer defense-in-depth is bypassed.

**Fix:** In `is_blocked_ip`, add a check for IPv4-mapped IPv6 addresses before the IPv6-specific checks. Use `Ipv6Addr::to_ipv4()` (stabilized in Rust 1.75) to extract the embedded IPv4 address and re-check it:

```rust
pub(crate) fn is_blocked_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_shared()  // CR-02: 100.64.0.0/10
        }
        IpAddr::V6(v6) => {
            // CR-01: convert IPv4-mapped IPv6 to IPv4 before checks
            if let Some(v4) = v6.to_ipv4() {
                return v4.is_private()
                    || v4.is_loopback()
                    || v4.is_link_local()
                    || v4.is_multicast()
                    || v4.is_broadcast()
                    || v4.is_unspecified()
                    || v4.is_shared();
            }
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
                || v6.is_unspecified()
        }
    }
}
```

Also add tests:

```rust
#[test]
fn is_blocked_ip_v4_mapped_ipv6_loopback() {
    // ::ffff:127.0.0.1 should be blocked
    let addr: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
    assert!(is_blocked_ip(&addr));
}

#[test]
fn is_blocked_ip_v4_mapped_ipv6_private() {
    // ::ffff:192.168.1.1 should be blocked
    let addr: IpAddr = "::ffff:192.168.1.1".parse().unwrap();
    assert!(is_blocked_ip(&addr));
}

#[test]
fn is_blocked_ip_v4_mapped_ipv6_public() {
    // ::ffff:8.8.8.8 should NOT be blocked
    let addr: IpAddr = "::ffff:8.8.8.8".parse().unwrap();
    assert!(!is_blocked_ip(&addr));
}
```

---

### CR-02: RFC 6598 shared address space (`100.64.0.0/10`) not blocked in SSRF IP check

**File:** `crates/jadepaw-wasm/src/host/network.rs:306-322`
**Issue:** The `is_blocked_ip` function for IPv4 checks `is_private()`, `is_loopback()`, `is_link_local()`, `is_multicast()`, `is_broadcast()`, and `is_unspecified()`, but does NOT check `is_shared()`. The `is_shared()` method (Rust 1.63+) covers RFC 6598 Carrier-Grade NAT address space `100.64.0.0/10`.

The shared address space is typically used by ISPs for CGNAT and cloud environments for internal networking (AWS uses it for VPC endpoints in some regions, GCP uses it for certain internal services). A host may have internal infrastructure accessible via these addresses.

**Impact:** An attacker with a whitelisted domain that resolves (or is coerced to resolve) to an address in `100.64.0.0/10` could reach the host's CGNAT/internal network. Combined with IPv4-mapped IPv6 (CR-01), this widens the SSRF attack surface.

**Fix:** Add `v4.is_shared()` to the IPv4 check chain in `is_blocked_ip`:

```rust
IpAddr::V4(v4) => {
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_multicast()
        || v4.is_broadcast()
        || v4.is_unspecified()
        || v4.is_shared()  // 100.64.0.0/10
}
```

Add test:

```rust
#[test]
fn is_blocked_ip_shared_address_space() {
    // RFC 6598 CGNAT range should be blocked
    assert!(is_blocked_ip(&"100.64.0.1".parse::<IpAddr>().unwrap()));
    assert!(is_blocked_ip(&"100.127.255.254".parse::<IpAddr>().unwrap()));
}
```

---

## Warnings

### WR-01: `parse_next_action` produces malformed tool name when `ACTION:` is immediately followed by whitespace and `(`

**File:** `crates/jadepaw-agent/src/llm.rs:237-281`
**Issue:** When the LLM produces `ACTION: (args)` (with a space between `ACTION:` and `(`), the parser behavior is incorrect:

1. Line 238: `action_str = "(args)".trim()` = `"(args)"`
2. Line 243: `action_str.find('(')` = `Some(0)` (found at position 0)
3. Line 244: `tool = action_str[..0].trim()` = `""` (empty string)
4. `tool.is_empty()` is true, so we skip to the fallback at lines 273-281
5. Line 274: `tool = "(args)"` (the trimmed action_str, which has no tool name)
6. Line 276: `!(args).is_empty()` is true → returns `Act { tool: "(args)", args: "" }`

The tool name `"(args)"` is obviously invalid and will result in an `UNKNOWN_TOOL` error from `ToolRegistry`, but the error message will be confusing ("Unknown tool: '(args)'"). The correct behavior should be to recognize that no tool name precedes the opening parenthesis and fall through to `ContinueThinking`.

**Fix:** Check that the tool name (text before `(`) is non-empty before attempting paren matching. If the `(` is at position 0 of `action_str`, skip to the `ContinueThinking` fallback:

```rust
if let Some(paren_pos) = action_str.find('(') {
    let tool = action_str[..paren_pos].trim().to_string();
    if tool.is_empty() {
        // Parenthesis found but no tool name before it.
        // Fall through to ContinueThinking (WR-01).
    } else {
        // ... existing paren matching logic ...
    }
}
```

Alternatively, modify the fallback at line 273 to check if the string starts with `(`:

```rust
// Fallback: treat entire string as tool name with empty args
let tool = action_str.trim().to_string();
if !tool.is_empty() && !tool.starts_with('(') {
    return LlmDirective::Act { thought, tool, args: String::new() };
}
```

---

### WR-02: `resolve_and_check_ssrf` performs separate DNS resolution from reqwest, but results are not pinned — TOCTOU window exists

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:259-275`
**Issue:** The SSRF IP check at line 265 calls `resolve_and_check_ssrf(&host)`, which independently resolves the hostname via `tokio::net::lookup_host`. The validated addresses are stored in `addrs` but never used for the actual reqwest request. At line 275, `let _ = &addrs;` is followed by a TODO comment documenting the double-DNS performance cost.

The actual reqwest request at lines 295-297 uses `url_str` directly, causing reqwest to perform its own DNS resolution internally. The TOCTOU window between the SSRF DNS resolution and reqwest's DNS resolution means:

1. A DNS rebinding attack could return a public IP during the SSRF check (line 265) and a private/internal IP during reqwest's resolution (line 338)
2. The SSRF IP check runs once, but the connection target is determined by reqwest's resolution

**Impact:** DNS rebinding window is documented as an accepted risk per the module docs (lines 67-68), and this is a known limitation. However, the severity is elevated because the SSRF check is effectively an advisory check that doesn't constrain the actual connection target.

**Fix:** The code already has a TODO at lines 271-274 documenting the proper fix: use `reqwest::ClientBuilder::dns_resolver()` with a custom resolver that uses the pre-validated addresses. In the short term, ensure the risk is documented in the security threat model. This is flagged as a WARNING rather than CRITICAL because (a) DNS rebinding requires the attacker to control a whitelisted domain's DNS, and (b) the domain whitelist is the primary defense.

---

### WR-03: SSRF IP check logic duplicated between `host/network.rs` and `tool_impls/http_tool.rs`

**File:** `crates/jadepaw-wasm/src/host/network.rs:198-235` and `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:69-109`
**Issue:** Both `http_request_host_fn` in `network.rs` and `resolve_and_check_ssrf` in `http_tool.rs` contain nearly identical DNS resolution + IP-checking logic (5-second timeout, `lookup_host`, iterate IPs, call `is_blocked_ip`). The error handling differs slightly: the host function returns `-1` for all failures, while the tool function returns structured `ToolResult::Error` variants with different error codes.

**Impact:** Any future change to SSRF IP check logic (e.g., adding `is_shared()` coverage from CR-02, or fixing CR-01's IPv4-mapped IPv6) must be applied in two places. This is a maintenance hazard. The host function path (`network.rs`) already has the domain whitelist check (line 104), the scheme validation (lines 140-150), and the SSRF IP check all inline, creating substantial code duplication with `HttpRequestTool`.

**Fix:** Extract the shared SSRF resolution logic into a common function, e.g., in `network.rs`, and have both call sites use it. The `resolve_and_check_ssrf` in `http_tool.rs` already has better error types; consider making it pub(crate) and having the host function also call it (translating the structured errors to the `-1` return convention).

---

## Info

### IN-01: `domain_matches` does not validate that pattern format is well-formed

**File:** `crates/jadepaw-wasm/src/capability/mod.rs:104-124`
**Issue:** The `domain_matches` function accepts any string as a pattern without validation. Patterns like `*.*.example.com` (multiple wildcards) or `*` (bare star) behave unexpectedly. `*.*.example.com` would strip prefix `*.` to get `*.example.com`, then check `domain.ends_with("*.example.com")` which would never match any domain (since the `*` is a literal character in the suffix). The `strip_prefix("*.")` at line 116 only strips the first `*.`.

**Impact:** Administrators might configure `*.*.example.com` expecting it to match two levels of subdomains, but it would silently match nothing. The bare `*` pattern (line 106) matches anything, which could be unintentionally broad.

**Fix:** Add validation at pattern registration time (in `DomainPattern::new()` or at registry startup) that rejects patterns containing internal `*` characters that are not at the beginning. Accept only `*`, `exact.domain.com`, `*.example.com` (single wildcard subdomain) formats. Alternatively, add a tracing warning for unrecognized patterns.

---

### IN-02: `host_functions.rs` test does not exercise `http_request` trait method

**File:** `crates/jadepaw-core/tests/host_functions.rs:60-89`
**Issue:** The `test_host_functions_trait_result_types` test verifies the `Result` types for `log_message`, `file_read`, and `file_write`, but does not test `http_request`. The `TestHostFn` struct implements `http_request` (lines 37-48), but the test only exercises three of the four methods.

**Impact:** Any future signature change to `http_request` that breaks the return type tuple `(u16, HashMap<String, String>, Vec<u8>)` would not be caught by this type-level test, potentially causing compilation failures only in downstream crates that use the `HostFunctions` trait.

**Fix:** Add an `http_request` invocation to the test (as previously noted in prior review IN-01, still applicable):

```rust
let result: Result<(u16, std::collections::HashMap<String, String>, Vec<u8>)> =
    rt.block_on(host.http_request(
        "GET".into(),
        "https://example.com".into(),
        std::collections::HashMap::new(),
        None,
    ));
assert!(result.is_err());
```

---

_Reviewed: 2026-06-03T22:10:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_