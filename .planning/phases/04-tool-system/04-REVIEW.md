---
phase: 04-tool-system
reviewed: 2026-06-04T19:00:00Z
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
  - crates/jadepaw-agent/tests/agent_loop.rs
findings:
  critical: 1
  warning: 4
  info: 2
  total: 7
status: issues_found
---

# Phase 04: Code Review Report (Fresh Adversarial Review)

**Reviewed:** 2026-06-04T19:00:00Z
**Depth:** standard
**Files Reviewed:** 22
**Status:** issues_found

## Summary

This is a fresh adversarial review of the Phase 04 (tool-system) codebase, conducted against the current state with all six prior fixes applied (CR-01 saturating_add, WR-01 MaxIterations SSE event, WR-02 dual tracking variables, WR-03 u64::try_from, WR-04 unused session_id field, WR-05 fa_pos naming). The review covers 22 source files across `jadepaw-core`, `jadepaw-agent`, and `jadepaw-wasm`.

The shot-through design -- Tool trait / ToolRegistry / HostFunctions three-layer separation with capability gating centralized in Registry -- is architecturally sound. SSRF defense-in-depth (scheme validation, domain whitelist, IP-layer check, redirect limiting, body cap, timeout) has multiple overlapping layers.

This fresh review found **1 critical** issue (resource leak via per-call reqwest Client construction in `http_request_host_fn`) and several warnings/info items not covered by the prior review.

**Previously fixed issues (all verified as correctly applied):**
- CR-01: `checked_add` refactoring in bounds check closure
- WR-01: `ReActStep::Error` event sent before MaxIterations exit
- WR-02: Single `truncated` flag replacing dual `total`/`buf.len()` tracking
- WR-03: `u64::try_from` replacing `as u64` for Duration conversion
- WR-04: Removed unused `session_id` fields from FileReadTool/FileWriteTool
- WR-05: Renamed `fa` to `final_answer_pos` in fallback branch
- IN-01 through IN-03: Acknowledged as info-level

## Critical Issues

### CR-01: `http_request_host_fn` creates a new `reqwest::Client` on every Wasm guest call, leaking OS resources

**File:** `crates/jadepaw-wasm/src/host/network.rs:233-243`
**Issue:** The `http_request_host_fn` (the Wasm guest-host boundary function) builds a fresh `reqwest::Client` on **every single invocation** from the guest:

```rust
let client = match reqwest::Client::builder()
    .redirect(redirect::Policy::limited(1))
    .timeout(Duration::from_secs(30))
    .build()
{
    Ok(c) => c,
    Err(e) => {
        warn!(%session_id, "http_request: failed to build reqwest client: {}", e);
        return -1;
    }
};
```

Each `reqwest::Client` allocates:
- An internal connection pool (by default up to 5 idle connections per host)
- A DNS resolver 
- TLS session cache

In a hostile scenario (malicious guest Wasm calling `http_request` in a tight loop with a 30s timeout), each call constructs and discards a full client with connection pool. The `Drop` of the old client (line 278: `drop(response)` followed by the `client` going out of scope at the end of the async block) should clean up connections, but the constant allocation/deallocation cycle of TLS contexts and connection pools under high-frequency calls constitutes a resource leak in practice -- it can exhaust file descriptors or TLS handshake slots on the host.

Compare this to `HttpRequestTool::call()` (http_tool.rs lines 59-65), which builds the client once in `build_http_client()` and stores it in the struct. The tool-level path reuses the same client across all calls.

**Why this is critical:** A malicious or buggy guest Wasm module can trigger thousands of `http_request` calls per second. Each call constructs a full reqwest client, potentially exhausting OS ephemeral ports, TLS handshake slots, or memory. This is a denial-of-service vector against the host, not just the guest.

**Fix:** Construct the reqwest client once (at Engine or Pool level) and pass a shared reference to the host function. The host function should receive an `Arc<reqwest::Client>` from the call context:

```rust
// In the linker registration path, store Arc<reqwest::Client> in SessionState
// or pass via Store data. Example:
pub fn http_request_host_fn(
    mut caller: Caller<'_, SessionState>,
    method_ptr: i32, method_len: i32,
    url_ptr: i32, url_len: i32,
    headers_ptr: i32, headers_len: i32,
    body_ptr: i32, body_len: i32,
) -> Box<dyn Future<Output = i32> + Send + '_> {
    Box::new(async move {
        let state = caller.data();
        let client = &state.http_client;  // stored once on SessionState
        // ... rest of function uses client instead of building a new one
    })
}
```

This requires adding `http_client: reqwest::Client` (or `Arc<reqwest::Client>`) to `SessionState` and initializing it during session creation.

---

## Warnings

### WR-01: `HttpRequestTool::Default::default()` panics on TLS initialization failure

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:188-192`
**Issue:** The `Default` impl calls `Self::new().expect("HttpRequestTool::default() requires working TLS")`. If the runtime environment lacks TLS support (e.g., a minimal container without CA certificates or rustls feature misconfiguration), this panics at `Default` construction time, not at the call site. The `new()` method is correctly fallible (`-> anyhow::Result<Self>`), but `Default` converts a recoverable error into a fatal panic.

In practice, code that calls `HttpRequestTool::default()` or uses `#[derive(Default)]` on a struct containing `HttpRequestTool` will crash the process if TLS initialization fails, rather than propagating the error for the caller to handle gracefully.

**Fix:** Either:
1. Remove the `Default` impl entirely and require callers to use the fallible `new()`, or
2. Make `Default` return a result via a separate trait:
```rust
// Option A: Remove Default impl, keep only new()
// Option B: Lazy-initialize the client on first call
impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            tracing::error!("HttpRequestTool::default() failed: {}", e);
            // Return a degraded tool that returns errors on every call
            Self {
                client: None,  // requires Option<reqwest::Client>
                allowed_methods: vec!["GET".into(), "POST".into(), "PUT".into(), "PATCH".into(), "DELETE".into()],
            }
        })
    }
}
```

---

### WR-02: `ToolRegistry::call_tool()` performs two independent `name_index` lookups creating a TOCTOU inconsistency window

**File:** `crates/jadepaw-agent/src/tool_registry.rs:107-137`
**Issue:** `call_tool()` calls `self.get_by_name(name)` at line 108, which internally does `self.name_index.get(name)` at line 81. Then at line 125, it does another `self.name_index.get(name)` to retrieve the `ToolId` for the capability check.

If concurrent code removes the tool entry from `name_index` between lines 108 and 125 (e.g., another thread calling a hypothetical `unregister()` method), the second lookup returns `None` and the code returns `INTERNAL_ERROR`. Even without explicit unregistration, the pattern is fragile because it relies on the atomicity of two separate `DashMap::get` operations.

The `get_by_name()` result at line 108 already retrieved the tool, but its `ToolId` is lost because `get_by_name` returns `Arc<dyn Tool>`, not `(ToolId, Arc<dyn Tool>)`.

**Fix:** Refactor `get_by_name` to return the `ToolId` alongside the tool, eliminating the second lookup:
```rust
pub fn get_by_name(&self, name: &str) -> Option<(ToolId, Arc<dyn Tool>)> {
    let id = self.name_index.get(name)?;
    let tool = self.tools.get(&*id).map(|entry| Arc::clone(entry.value()))?;
    Some((*id, tool))
}

// In call_tool():
let (tool_id, tool) = match self.get_by_name(name) {
    Some(t) => t,
    None => {
        return ToolResult::from_error("UNKNOWN_TOOL", ...);
    }
};
```

---

### WR-03: `http_request_host_fn` does not validate or reject forbidden request headers unlike `HttpRequestTool::call()`

**File:** `crates/jadepaw-wasm/src/host/network.rs:254-256` vs `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:324-351`
**Issue:** The `HttpRequestTool::call()` implementation (lines 324-351) blocks dangerous request headers (`host`, `content-length`, `transfer-encoding`, `proxy-authorization`, `connection`, `expect`) and rejects header values containing CR/LF sequences (header injection prevention). However, the `http_request_host_fn` (lines 254-256) passes all headers through without any filtering:

```rust
for (key, value) in &headers {
    request = request.header(key.as_str(), value.as_str());
}
```

The rationale is that the host function's `headers` come from guest memory and the guest is sandboxed, so a guest setting `Host: internal-service` is a self-inflicted attack. However, the `Host` header specifically can interact with virtual hosting on the target server, and `Transfer-Encoding` could potentially interact with front-end proxies in unexpected ways. While these headers were set by the guest itself (not an external attacker), the inconsistency creates a scenario where:
1. The Tool-level API (agent path) correctly blocks dangerous headers
2. The Wasm-level API (guest direct path) silently passes them

This asymmetry means a guest that has capability to call `http_request` directly (bypassing the Tool layer) can set headers that the Tool layer would reject.

**Mitigation:** The guest is already sandboxed and the domain whitelist is checked before any request. The guest controlling headers is part of its own execution. However, for defense-in-depth consistency, the same header filtering should apply in both paths.

**Fix:** Extract the header filtering logic into a shared function used by both `http_request_host_fn` and `HttpRequestTool::call()`:
```rust
// In host/network.rs, shared helper:
pub(crate) const FORBIDDEN_REQUEST_HEADERS: &[&str] = &[
    "host", "content-length", "transfer-encoding",
    "proxy-authorization", "connection", "expect",
];

pub(crate) fn filter_request_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers.iter()
        .filter(|(k, v)| {
            let lower = k.to_lowercase();
            FORBIDDEN_REQUEST_HEADERS.contains(&lower.as_str())
                || v.contains('\r') || v.contains('\n')
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
```

---

### WR-04: `extract_host_from_url` strips userinfo but `http_request_host_fn` passes the raw URL (with credentials) to reqwest

**File:** `crates/jadepaw-wasm/src/host/network.rs:100, 254`
**Issue:** The `http_request_host_fn` correctly strips userinfo from the URL for the domain capability check (line 100: `let domain = extract_host_from_url(url)`). However, the raw `url` string (which may contain `user:password@`) is passed directly to reqwest at line 254:

```rust
let mut request = client.request(reqwest_method, url);
```

reqwest will parse the userinfo from the URL and send it as a basic auth `Authorization` header. The guest code is sandboxed, so the credentials come from the guest itself -- not from host secrets. However, if a Skill author embeds third-party API credentials in their Wasm guest code (bad practice but plausible), those credentials would be sent to the target server in cleartext URL form.

This is not a vulnerability in the host's security model (the guest controls its own request content), but it is a privacy/data-exfiltration concern: the userinfo portion is logged by intermediate proxies and servers, making credential leakage more likely than if credentials were sent via the `Authorization` header.

**Fix:** Strip userinfo from the URL before passing to reqwest. If userinfo is present, return an error or strip it silently:
```rust
// After extracting domain, strip userinfo from the actual URL used for the request
let request_url = if url.contains('@') && url.contains("://") {
    // Strip userinfo portion: http://user:pass@host/path -> http://host/path
    // This is a best-effort; more robust URL parsing should use the url crate.
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            format!("{}://{}", &url[..scheme_end], &after_scheme[at_pos + 1..])
        } else {
            url.to_string()
        }
    } else {
        url.to_string()
    }
} else {
    url.to_string()
};
```

Alternatively, reject URLs containing userinfo with an explicit error.

---

## Info

### IN-01: `stream_llm_response` parameter named `close_signal` is confusing -- it is actually an `mpsc::Sender`

**File:** `crates/jadepaw-agent/src/llm.rs:162`
**Issue:** The function signature declares `close_signal: &mpsc::Sender<ReActStep>` as a parameter. The doc comment says "The close_signal parameter is used to detect channel close (graceful early termination if the SSE consumer disconnects)." The actual usage at line 188 is `close_signal.is_closed()` -- it is only used to check if the channel is closed, and never used for sending.

The issue is that the type `&mpsc::Sender<ReActStep>` is used both at the call site (`react_loop` passes `tx`) and in the function body, but the function only needs a `Receiver` or a simpler `Closed` check mechanism. The name `close_signal` describes the purpose well, but reading the function body requires the reader to understand that `close_signal.is_closed()` checks the sender's connected state.

**Fix:** Rename the parameter to better reflect its purpose. Consider extracting a simple `ConnectionState` wrapper:
```rust
pub struct ConnectionState {
    tx: mpsc::Sender<ReActStep>,
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        !self.tx.is_closed()
    }
}
```
Then `stream_llm_response` receives `&ConnectionState` and checks `conn.is_connected()`.

---

### IN-02: `resolve_and_check_ssrf` helper shadows the `host` field in `SsrfDnsError::Blocked` variant

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:79-115`, `crates/jadepaw-wasm/src/host/network.rs:292-298`
**Issue:** The `resolve_and_check_ssrf` function at http_tool.rs line 79 takes a parameter `host: &str`. The error mapping at line 97 matches `SsrfDnsError::Blocked { host: _, ip }` -- the `host` field from the error is bound to `_` (unused) because the function already has `host` in scope from the parameter. This works correctly but is a subtle naming collision that could confuse readers.

The error message at line 100-103 uses the parameter `host` (from the function signature) rather than the error's `host` field. Since `resolve_and_check_ssrf_addr` (the callee) passes the same hostname into the error, the two values are identical, so there is no correctness issue. But the pattern is fragile: if the callee ever normalizes the hostname before storing it in the error, the error message would show the original (non-normalized) host while the error struct carries the normalized version.

**Fix:** Destructure with an explicit ignore comment:
```rust
SsrfDnsError::Blocked { host: blocked_host, ip } => ToolResult::Error {
    code: "SSRF_BLOCKED".to_string(),
    message: format!(
        "Host '{}' resolved to blocked IP address {} ...",
        blocked_host, ip
    ),
    retryable: false,
},
```

---

_Reviewed: 2026-06-04T19:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_