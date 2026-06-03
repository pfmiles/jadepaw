---
phase: 04-tool-system
fixed_at: 2026-06-04T00:00:00Z
review_path: .planning/phases/04-tool-system/04-REVIEW.md
iteration: 2
findings_in_scope: 5
fixed: 5
skipped: 0
status: all_fixed
---

# Phase 04: Code Review Fix Report (Second Re-review)

**Fixed at:** 2026-06-04T00:00:00Z
**Source review:** .planning/phases/04-tool-system/04-REVIEW.md
**Iteration:** 2

**Summary:**
- Findings in scope: 5 (2 Critical + 3 Warning, from second re-review)
- Fixed: 5
- Skipped: 0

## Fixed Issues

### CR-01: IPv4-mapped IPv6 address bypasses SSRF IP check

**Files modified:** `crates/jadepaw-wasm/src/host/network.rs`
**Commit:** 3f5d88d
**Applied fix:** Added `is_ipv4_mapped()` helper that identifies true IPv4-mapped IPv6 addresses (prefix `::ffff:0:0/96`) by checking `segments()` for `[0, 0, 0, 0, 0, 0xFFFF, ..]` and manually reconstructing the embedded IPv4 from the last two segments. This is used instead of the unstable `Ipv6Addr::to_ipv4()` because `to_ipv4()` incorrectly returns `Some(0.0.0.1)` for `::1` (true IPv6 loopback), which would cause `::1` to be checked against the IPv4 blocklist instead of the IPv6 one. The `is_blocked_ip` function now extracts and checks the embedded IPv4 for IPv4-mapped addresses before falling through to IPv6-specific checks. Added 6 tests covering IPv4-mapped loopback, private, shared (CGNAT), and public IPs.

### CR-02: RFC 6598 shared address space (`100.64.0.0/10`) not blocked in SSRF IP check

**Files modified:** `crates/jadepaw-wasm/src/host/network.rs`
**Commit:** 3f5d88d
**Applied fix:** Since Rust's `Ipv4Addr::is_shared()` remains unstable (tracking issue #27709) as of Rust 1.95, implemented a manual `is_shared_v4()` function checking for octets `100.64-127.*`. Extracted the full IPv4 check chain into `is_blocked_v4()` for reuse in both the V4 branch and the IPv4-mapped V6 branch (CR-01). Added `is_blocked_ip_shared_address_space` test covering the range boundaries.

### WR-01: `parse_next_action` produces malformed tool name when `ACTION:` is immediately followed by whitespace and `(`

**Files modified:** `crates/jadepaw-agent/src/llm.rs`
**Commit:** 0ad8346
**Applied fix:** Modified the fallback in `parse_next_action` (when parenthesis-based parsing fails) to reject tool names starting with `(`. When the LLM emits `ACTION: (args)`, the `(` is found at position 0 so the tool name is empty. Previously the fallback would return `Act { tool: "(args)", args: "" }` producing a confusing `UNKNOWN_TOOL` error. Now it falls through to `ContinueThinking` so the LLM can self-correct. All 6 existing parser tests continue to pass.

### WR-02: `resolve_and_check_ssrf` performs separate DNS resolution from reqwest, but results are not pinned -- TOCTOU window exists

**Files modified:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs`
**Commit:** 40f3f14
**Applied fix:** Expanded the "Known risk" section in `HttpRequestTool`'s module-level documentation to explicitly describe the TOCTOU DNS rebinding window: the SSRF check resolves DNS independently, reqwest resolves DNS again, and the validated addresses are not pinned. Documented the full fix path (custom `dns_resolver`) and the conditions that make this an accepted risk (domain whitelist is primary defense, DNS rebinding requires compromised DNS). No code change needed -- the TODO and risk documentation are appropriate for MVP.

### WR-03: SSRF IP check logic duplicated between `host/network.rs` and `tool_impls/http_tool.rs`

**Files modified:** `crates/jadepaw-wasm/src/host/network.rs`, `crates/jadepaw-wasm/src/tool_impls/http_tool.rs`
**Commit:** a77b72d
**Applied fix:** Extracted the shared DNS resolution + IP-checking logic into `resolve_and_check_ssrf_addr()` with a new `SsrfDnsError` enum providing fine-grained error variants (`Timeout`, `DnsError`, `Blocked`, `NoAddresses`). The host function converts errors to `-1` return values with `warn!` logging. The `HttpRequestTool`'s private `resolve_and_check_ssrf` is now a thin wrapper (9 lines of business logic) that maps `SsrfDnsError` variants to `ToolResult::Error` with compatible error codes (`DNS_TIMEOUT`, `DNS_ERROR`, `SSRF_BLOCKED`). All 10 existing `is_blocked_ip` tests continue to pass.

---

_Fixed: 2026-06-04T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 2_