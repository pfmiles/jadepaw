---
phase: 04-tool-system
reviewed: 2026-06-04T00:00:00Z
depth: standard
files_reviewed: 24
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-agent/src/tool_registry.rs
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/tool.rs
  - crates/jadepaw-core/src/capabilities.rs
  - crates/jadepaw-core/src/error.rs
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
  warning: 4
  info: 3
  total: 9
status: issues_found
---

# Phase 04: Code Review Report (Third Re-review)

**Reviewed:** 2026-06-04
**Depth:** standard
**Files Reviewed:** 24
**Status:** issues_found

## Summary

This is the fourth adversarial review of Phase 04 (tool-system). Previous review rounds addressed CR-01/CR-02 (SSRF bypasses: userinfo stripping, IPv4-mapped IPv6, RFC 6598 shared address space), WR-01 (IPv6 bracket notation), WR-02 (silent header drops, TOCTOU documentation), WR-03 (SSRF logic duplication resolved into shared function). These fixes are verified correct in the current code.

This re-review identifies two new behavioral correctness issues (BLOCKER tier), four warnings covering logic edge cases and error handling miscategorization, and three info items on dead code, structural duplication, and magic numbers.

## Critical Issues

### CR-01: Silent loss of FINAL ANSWER when ACTION parsing fails in a response containing both directives

**File:** `crates/jadepaw-agent/src/llm.rs:229-296`
**Issue:** In `parse_next_action`, when both `ACTION:` and `FINAL ANSWER:` appear in the LLM response with ACTION textually before FINAL ANSWER (`act < fa`), the match arm `(_, Some(act))` (line 237) handles the ACTION. If ACTION parsing fails -- empty tool name, unbalanced parentheses, or `ACTION: (args)` with whitespace before the paren -- the code falls through to the implicit `(None, None)` arm and returns `ContinueThinking`. The subsequent `FINAL ANSWER:` directive is never examined.

The reverse case works correctly: when `FINAL ANSWER` appears first (`fa < act`) and its answer is empty, the code falls through to the `(_, Some(act))` arm and the ACTION is properly parsed. The asymmetry is the bug.

**Reproduction:**
```
THOUGHT: Let me try something...
ACTION:   (query="Paris")    # tool name empty due to whitespace before paren
FINAL ANSWER: The weather is sunny.
```
Expected: `Finish` with answer.
Actual: `ContinueThinking` -- agent restarts the loop, never producing a final answer.

**Impact:** When the LLM produces a malformed action followed by a valid final answer, the agent silently loops until it hits the iteration limit or the LLM happens to produce a parseable response. The user sees no answer and a large execution trace of wasted LLM calls.

**Fix:**
```rust
(_, Some(act)) => {
    let mut parsed_action = false;
    let action_str = after_thought[act + "ACTION:".len()..].trim();
    // ... existing paren-depth-parsing logic (lines 243-271) ...
    if let Some(paren_pos) = action_str.find('(') {
        let tool = action_str[..paren_pos].trim().to_string();
        let inner = &action_str[paren_pos + 1..];
        let mut depth = 1usize;
        let mut close_pos = None;
        for (i, ch) in inner.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => { depth -= 1; if depth == 0 { close_pos = Some(i); break; } }
                _ => {}
            }
        }
        if let Some(pos) = close_pos {
            let args = inner[..pos].trim().to_string();
            if !tool.is_empty() {
                parsed_action = true;
                return LlmDirective::Act { thought, tool, args };
            }
        }
    }
    if !parsed_action {
        // ... existing no-paren fallback logic (lines 273-287) ...
        let tool = action_str.trim().to_string();
        if !tool.is_empty() && !tool.starts_with('(') {
            parsed_action = true;
            return LlmDirective::Act { thought, tool, args: String::new() };
        }
    }
    // CR-01 fix: if ACTION parsing failed and a FINAL ANSWER is known
    // to follow (act_pos < fa_pos), try to extract it before giving up.
    if !parsed_action {
        if let Some(fa) = fa_pos {
            let answer = after_thought[fa + "FINAL ANSWER:".len()..]
                .trim().to_string();
            if !answer.is_empty() {
                return LlmDirective::Finish { thought, answer };
            }
        }
    }
    // Truly no actionable directive: continue thinking.
}
```

---

### CR-02: `reqwest::Client` construction panics at agent startup via `.expect()` on an infallible-looking builder

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:58-64`
**Issue:** The function `build_http_client()` calls `.expect("reqwest Client builder should not fail with valid config")` on `reqwest::Client::builder()...build()`. This function is called from `HttpRequestTool::new()` (line 129), which is invoked at agent startup. If the system's TLS backend fails to initialize -- possible on restricted containers, broken OpenSSL installations, or systems where `/dev/urandom` is unavailable -- the entire agent process panics instead of returning a graceful error.

Unlike the `unreachable!()` on line 242 of `network.rs` (which is guarded by a validated method enum), this `.expect()` is on an **I/O-dependent operation** that can fail in production environments. In a multi-tenant system serving hundreds of agents, a single broken node's TLS initialization crashes the whole process.

**Fix:** Make `new()` fallible:
```rust
pub fn new() -> Result<Self, anyhow::Error> {
    Ok(Self {
        client: reqwest::Client::builder()
            .redirect(redirect::Policy::limited(1))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to initialize HTTP client for HttpRequestTool")?,
        allowed_methods: vec![...],
    })
}

impl Default for HttpRequestTool {
    fn default() -> Self {
        Self::new().expect("HttpRequestTool::default() requires working TLS")
    }
}
```

---

## Warnings

### WR-01: `unwrap_or_default()` silently discards tool input_schema serialization failures

**File:** `crates/jadepaw-agent/src/llm.rs:123`
**Issue:** In `build_system_prompt_with_tools()`, the `input_schema` field is serialized with:
```rust
serde_json::to_string(&t.input_schema).unwrap_or_default()
```
If `input_schema` contains a `serde_json::Value` that is non-serializable (e.g., `f64::NAN` or `f64::INFINITY`), `serde_json::to_string` returns an `Err`, and `unwrap_or_default()` silently yields `""`. The LLM prompt then contains `Parameters: ` with no schema, degrading tool call quality. The LLM will attempt to call the tool without knowing its parameter format.

**Fix:**
```rust
serde_json::to_string(&t.input_schema).unwrap_or_else(|e| {
    tracing::warn!(tool = %t.name, error = %e,
        "failed to serialize tool input_schema for prompt injection");
    format!("\"<serialization error: {}>\"", e)
})
```

---

### WR-02: `unwrap_or_default()` silently discards malformed guest headers in host function

**File:** `crates/jadepaw-wasm/src/host/network.rs:180`
**Issue:** In `http_request_host_fn`, the header JSON is deserialized with:
```rust
serde_json::from_str(s).unwrap_or_default()
```
If the guest module sends headers as valid UTF-8 that is **not** valid JSON (e.g., a comma-separated key=value string, or raw text), the headers are silently set to an empty `HashMap`. The HTTP request proceeds **without any headers**, potentially changing its behavior compared to the guest's intent. In a security-sensitive context (e.g., a guest-provided `Authorization` header intended for a specific API), this silent stripping is especially dangerous.

**Fix:** Return `-1` on invalid header JSON, so the guest is notified of the malformed input:
```rust
match serde_json::from_str(s) {
    Ok(h) => h,
    Err(e) => {
        warn!(%session_id, "http_request: invalid JSON in headers: {}", e);
        return -1;
    }
}
```

---

### WR-03: `unwrap_or(reqwest::Method::GET)` silently masks internal logic errors in HTTP tool

**File:** `crates/jadepaw-wasm/src/tool_impls/http_tool.rs:301`
**Issue:** The request dispatch uses:
```rust
method.parse::<reqwest::Method>().unwrap_or(reqwest::Method::GET)
```
The method string was already validated against `allowed_methods` on line 236 -- all entries in `allowed_methods` match valid `reqwest::Method` variants. Any parse failure here represents an **internal logic error** (e.g., the validated string was corrupted between the check and the parse). Using `unwrap_or` silently defaults to GET, turning a POST-with-body into a GET-without-body. This masks the bug and produces incorrect HTTP behavior.

**Fix:** Use an explicit match that makes the invariant visible:
```rust
let reqwest_method = match method.as_str() {
    "GET" => reqwest::Method::GET,
    "POST" => reqwest::Method::POST,
    "PUT" => reqwest::Method::PUT,
    "PATCH" => reqwest::Method::PATCH,
    "DELETE" => reqwest::Method::DELETE,
    _ => unreachable!("method validated against allowed_methods"),
};
```
This eliminates the parse step entirely and makes the match exhaustive at compile time.

---

### WR-04: Fuel reset failure miscategorised as `LoopErrorKind::LlmFailure`

**File:** `crates/jadepaw-agent/src/loop.rs:141-149`
**Issue:** The session store fuel reset error is mapped to `LoopErrorKind::LlmFailure`:
```rust
session.store_mut().set_fuel(1_000_000).map_err(|e| {
    loop_error(LoopErrorKind::LlmFailure {
        turn,
        source: anyhow::anyhow!("failed to set fuel on session store: {}", e),
    })
})?;
```
`LlmFailure` semantically implies an LLM API call failure (as used on lines 160-165 for actual LLM errors). A store-access failure is a different category -- it is an infrastructure error on the Wasm runtime side, not the LLM side. In `guard.rs`, both map to `InfrastructureError` (which is coincidentally correct), but the error classification is misleading for observability. Operators investigating LLM failure alerts would see store-access errors mixed in.

**Fix:** Either introduce a dedicated `LoopErrorKind::StoreFailure { turn, source }` variant, or rename `LlmFailure` to something broader like `InfrastructureFailure`. Minimally, ensure the error message clearly distinguishes the failure origin (the `.context()` on line 144 does this with `"failed to set fuel on session store"`).

---

## Info

### IN-01: `session_id` field stored but never read in `FileReadTool` and `FileWriteTool`

**File:** `crates/jadepaw-wasm/src/tool_impls/file_tool.rs:31-36, 149-155`
**Issue:** Both `FileReadTool` and `FileWriteTool` store `session_id: SessionId` as a field, but the `call()` method uses only the `_session_id: SessionId` parameter from the `Tool` trait signature. The stored `self.session_id` is never read. The `#[allow(dead_code)]` attribute on both structs suppresses the compiler warning. This suggests the field was intended for structured logging/audit but the logging was never implemented.

**Fix:** Either use `self.session_id` inside `call()` with `tracing::info!(%self.session_id, ...)` for audit logging, or remove the field from both structs and their constructors.

---

### IN-02: Duplicate `extract_host_from_url` tests across two crates

**File:** `crates/jadepaw-core/src/tool.rs:242-311` and `crates/jadepaw-wasm/src/host/network.rs:404-425`
**Issue:** Identical test cases for `extract_host_from_url` exist in both `jadepaw-core` (canonical implementation) and `jadepaw-wasm` (thin delegation wrapper). The wasm-side tests re-verify behavior that the core tests already cover. This duplicates maintenance burden: any new test case must be added to both locations.

**Fix:** Remove the `extract_host_*` test block from `network.rs` (lines 404-425), or add a comment documenting them as smoke tests for the delegation wrapper. The wasm crate's test focus should be `is_blocked_ip` and `resolve_and_check_ssrf_addr`.

---

### IN-03: Per-turn fuel budget value `1_000_000` is a magic number

**File:** `crates/jadepaw-agent/src/loop.rs:143`
**Issue:** The Wasm fuel reset uses the inline literal `1_000_000`:
```rust
session.store_mut().set_fuel(1_000_000)
```
The design document D-10 Pitfall 3 specifies this value, but it is not extracted as a named constant. If the fuel budget needs tuning per tenant or per agent complexity tier, the literal must be found and replaced across the codebase.

**Fix:**
```rust
/// Per-turn Wasm fuel budget in fuel units (D-10 Pitfall 3).
/// Each ReAct iteration resets the guest's fuel to this value to prevent
/// infinite loops and excessive compute consumption.
const PER_TURN_FUEL_BUDGET: u64 = 1_000_000;

// ...
session.store_mut().set_fuel(PER_TURN_FUEL_BUDGET)
```

---

_Reviewed: 2026-06-04_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_