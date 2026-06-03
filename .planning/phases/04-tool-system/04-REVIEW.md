---
phase: 04-tool-system
reviewed: 2026-06-03T00:00:00Z
depth: standard
files_reviewed: 21
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
  critical: 0
  warning: 3
  info: 3
  total: 6
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-06-03T00:00:00Z
**Depth:** standard
**Files Reviewed:** 21
**Status:** issues_found

## Summary

This is an adversarial re-review of Phase 04 (tool-system), which introduces the Tool abstraction layer spanning three crates: `jadepaw-core` (Tool trait, ToolResult, ToolDefinition), `jadepaw-agent` (ToolRegistry, ReAct loop tool dispatch, LLM parsing with balanced parens), and `jadepaw-wasm` (FileReadTool, FileWriteTool, HttpRequestTool, Wasm host function network support with SSRF protections).

The original review identified three critical, six warning, and four info-level findings. All three critical issues (CR-01 domain capability bypass, CR-02/CR-03 unbounded HTTP body reads) and four of the six warnings (WR-02 header injection, WR-03 wall-clock elapsed, WR-05 parser parens, WR-06 trait limitation documented) have been addressed. The fixes are present in the current code and look correct.

This re-review confirms no new critical bugs or security vulnerabilities. Three warnings remain that represent code quality and maintainability concerns rather than correctness issues: fragile hardcoded tool-name coupling in the registry (WR-01), duplicated URL host extraction logic (WR-02), and redundant DNS resolution in the HTTP tool (WR-03). Three info-level items cover minor dead code, redundancy, and error handling style.

## Warnings

### WR-01: Hardcoded tool name "http_request" in domain capability check creates fragile coupling

**File:** `crates/jadepaw-agent/src/tool_registry.rs:159`
**Issue:** The domain capability check for HTTP requests is gated on a string equality comparison `name == "http_request"`. This couples the ToolRegistry to a specific tool name string. If `HttpRequestTool::name()` is ever refactored to return a different value (e.g., `"http"` or `"fetch"`), the domain check silently stops applying — the compiler cannot detect this coupling. Similarly, if a second network-capable tool is registered (e.g., `WebSocketTool`), the check would need to be duplicated for the new tool name.

**Fix:** Use a shared constant for the tool name to ensure a single point of change:
```rust
// In http_tool.rs:
pub const TOOL_NAME: &str = "http_request";

// In tool_registry.rs:
use jadepaw_wasm::http_tool::TOOL_NAME as HTTP_REQUEST_TOOL_NAME;
...
if name == HTTP_REQUEST_TOOL_NAME {
    let state = session.store().data();
    if let Some(host) = extract_host_from_tool_args(&args) {
        if !state.can_access_domain(&host) {
            return ToolResult::from_error(
                "CAPABILITY_DENIED",
                &format!("Domain '{}' is not in the session's network capability whitelist.", host),
                false,
            );
        }
    }
}
```

A more robust long-term fix would be a tool-level metadata field or capability type enum that the registry inspects, but the constant approach is sufficient for the MVP.

---

### WR-02: Duplicated URL host extraction functions in tool_registry.rs and network.rs

**File:** `crates/jadepaw-agent/src/tool_registry.rs:193-217` and `crates/jadepaw-wasm/src/host/network.rs:295-320`
**Issue:** Two independent implementations of URL hostname extraction exist:
- `extract_host_from_tool_args()` in `tool_registry.rs` — used for the registry-level domain capability check
- `extract_host_from_url()` in `network.rs` — used by the `HttpRequestTool::validate_url()` method

Both implement identical string-processing logic (strip scheme at `://`, strip path at `/`/`?`/`#`, strip port at `:`), but they differ in implementation style: `tool_registry.rs` uses `.or_else()` chaining while `network.rs` uses `if let` / `else if` chains. The logic is functionally equivalent for normal URLs, but any future refinement to handle edge cases (percent-encoded hosts, IPv6 bracket notation, userinfo components) would need to be applied in two places, risking divergence.

**Fix:** Extract the canonical implementation to `jadepaw-core` (both `jadepaw-agent` and `jadepaw-wasm` already depend on it), then use it from both call sites:
```rust
// In jadepaw-core/src/tool.rs
pub fn extract_host_from_url(url: &str) -> &str {
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };
    let host_and_port = if let Some(idx) = after_scheme.find('/') {
        &after_scheme[..idx]
    } else if let Some(idx) = after_scheme.find('?') {
        &after_scheme[..idx]
    } else if let Some(idx) = after_scheme.find('#') {
        &after_scheme[..idx]
    } else {
        after_scheme
    };
    if let Some(idx) = host_and_port.find(':') {
        &host_and_port[..idx]
    } else {
        host_and_port
    }
}
```

Delete both the `extract_host_from_tool_args()` helper in `tool_registry.rs` and update `extract_host_from_url()` in `network.rs` to re-export from `jadepaw_core::tool::extract_host_from_url`.

---

### WR-03: HttpRequestTool performs redundant DNS resolution — once for SSRF check, once for reqwest

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:258-263`
**Issue:** The `call()` method calls `resolve_and_check_ssrf(&host)` (line 258) which resolves the hostname and validates all IPs against `is_blocked_ip()`. The resolved addresses are then discarded (`let _ = &addrs;` at line 263), and the actual HTTP request at line 316 goes through `reqwest`, which performs its own independent DNS resolution. This means every request triggers two DNS lookups, doubling the DNS-related latency (worst case: 2 x 5s timeout = 10s). The module-level documentation acknowledges the TOCTOU security window as accepted MVP risk, but does not mention the latency cost.

**Fix:** For the MVP, add a comment noting the performance implication:
```rust
// TODO(perf): SSRF IP check resolves DNS independently of reqwest, doubling DNS latency.
// Future optimization: pin resolved IPs to the reqwest connection to avoid re-resolution.
let addrs = resolve_and_check_ssrf(&host).await?;
let _ = &addrs;
```

Longer term, use `reqwest::ClientBuilder::dns_resolver()` or a custom `reqwest::dns::Resolve` implementation to inject the checked addresses directly into reqwest's connection pool.

---

## Info

### IN-01: Dead `session_id` field in FileReadTool and FileWriteTool structs

**File:** `crates/jadepaw-wasm/src/tool_impls/file_tool.rs:34-35, 153-154`
**Issue:** Both `FileReadTool` and `FileWriteTool` contain a `session_id: SessionId` field that is stored in `::new()` but never read by any method. The `Tool::call()` method receives `_session_id: SessionId` as a parameter, which shadows the struct field. The struct field provides no audit logging, authorization, or any other function — it is pure dead data. Additionally, both structs retain `#[allow(dead_code)]` annotations that are no longer needed (the structs are exported via `pub use` in `lib.rs:42-43`).

**Fix:** Remove the `session_id` field from both structs and their constructors. If audit logging is planned for a future phase, add it then. Remove the `#[allow(dead_code)]` annotations.

```rust
// FileReadTool — remove session_id:
pub struct FileReadTool {
    sandbox_root: PathBuf,
}
pub fn new(sandbox_root: PathBuf) -> Self {
    Self { sandbox_root }
}
```

---

### IN-02: Redundant name_index lookup in ToolRegistry::call_tool

**File:** `crates/jadepaw-agent/src/tool_registry.rs:107-124`
**Issue:** `call_tool()` performs two separate `name_index` lookups for the same tool name:
1. Line 107-108: `self.get_by_name(name)` — looks up `name_index`, then `tools`
2. Line 124-136: `self.name_index.get(name)` — looks up `name_index` again for the tool_id

The second lookup is redundant since the first one already validated that the tool exists. The code correctly handles the edge case where the second lookup returns `None` (by returning `INTERNAL_ERROR`), but this is dead error-handling — if the tool was found by `get_by_name`, it must exist in `name_index` by construction (no `deregister` method exists).

**Fix:** Look up the tool_id first, then use it to fetch the tool, eliminating the second lookup:
```rust
let tool_id = match self.name_index.get(name) {
    Some(id) => *id,
    None => {
        let available: Vec<String> =
            self.name_index.iter().map(|e| e.key().clone()).collect();
        return ToolResult::from_error(
            "UNKNOWN_TOOL",
            &format!("Unknown tool: '{}'. Available tools: {:?}", name, available),
            false,
        );
    }
};
let tool = match self.tools.get(&tool_id) {
    Some(entry) => Arc::clone(entry.value()),
    None => {
        return ToolResult::from_error(
            "INTERNAL_ERROR",
            &format!("Tool '{}' found in name_index but not in tools", name),
            false,
        );
    }
};
```

---

### IN-03: `build_http_client()` uses `.expect()` in shared library code

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:44-50`
**Issue:** The `build_http_client()` function uses `.expect("reqwest Client builder should not fail with valid config")` on the `reqwest::Client::builder().build()` result. While the no-argument builder configuration will not fail under normal conditions, `.expect()` (panics) in library code means a user application crashes if an unexpected environment issue (e.g., TLS backend initialization failure, missing system CA certificates) causes the builder to fail. This is called once during `HttpRequestTool` construction, not per-request, so the practical risk is minimal, but panicking in library code is a code quality anti-pattern.

**Fix:** Either propagate the error or document the justification:
```rust
fn build_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(redirect::Policy::limited(1))
        .timeout(Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}
```

Alternatively, if the panic is intentional (fail-fast during initialization), add a comment explaining why:
```rust
// Panics acceptable: called once at tool construction time during application
// startup. A failure here indicates a fundamentally broken runtime (missing
// TLS backend or system certificates), and panicking is the correct response
// to prevent silent failures.
```

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_