---
phase: 03-agent-runtime
reviewed: 2026-06-01T23:45:00Z
depth: standard
files_reviewed: 12
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/termination.rs
  - crates/jadepaw-core/Cargo.toml
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/error.rs
  - crates/jadepaw-core/src/guest_exports.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/tests/agent_types.rs
findings:
  critical: 2
  warning: 4
  info: 2
  total: 8
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-06-01T23:45:00Z
**Depth:** standard
**Files Reviewed:** 12
**Status:** issues_found

## Summary

Reviewed the agent runtime implementation spanning `jadepaw-agent` (ReAct loop, LLM integration, termination guard, SSE streaming) and `jadepaw-core` (shared types, error types, guest exports).

The overall architecture is sound: types are correctly separated across crates (`jadepaw-core` carries zero wasmtime dependency), the ReAct loop skeleton is clean, and the `tokio::select!` termination guard pattern is correct. However, 2 critical semantic issues and 4 warnings were identified. The most impactful problems are (1) the indiscriminate mapping of all agent loop errors to `WasmTrap` -- including LLM API errors, channel closures, and pool acquisition failures -- and (2) sub-second timeout truncation in the wall-clock termination reason. Both would prevent callers from correctly diagnosing what went wrong in production.

---

## Critical Issues

### CR-01: `AgentTerminationReason::WasmTrap` used as a catch-all for non-Wasm failures

**File:** `crates/jadepaw-agent/src/guard.rs:74-100`, `crates/jadepaw-agent/src/lib.rs:79-85,122-129`
**Issue:**
The `WasmTrap` variant is used indiscriminately for three fundamentally different failure categories:

| Actual failure | Where it happens | Termination reason used |
|---|---|---|
| LLM API call failed (network, auth, rate limit) | `guard.rs:74-82` | `WasmTrap` |
| Output channel closed (client disconnect) | `guard.rs:83-90` | `WasmTrap` |
| Any unknown loop error | `guard.rs:91-99` | `WasmTrap` |
| Pool acquisition failed | `lib.rs:79-84` | `WasmTrap` |
| No `Finished` step found in trace | `lib.rs:122-128` | `WasmTrap` |

The code self-documents the problem on line 75: "LLM failures are not Wasm traps -- surface as a WasmTrap". A caller receiving `WasmTrap` cannot distinguish an actual guest sandbox violation from a transient network error, making operational monitoring and retry logic impossible to implement correctly.

**Fix:** Introduce a dedicated variant for infrastructure errors that are not Wasm traps:

```rust
// In agent_types.rs, add to AgentTerminationReason:
InfrastructureError {
    /// Human-readable error context.
    reason: String,
    /// The turn number (0-indexed) on which the error occurred.
    turn: u32,
},
```

Then update call sites:

```rust
// guard.rs:74-82 — LLM failures
JadepawError::agent_terminated(
    AgentTerminationReason::InfrastructureError {
        reason: format!("LLM error: {}", err_msg),
        turn,
    },
)

// guard.rs:83-90 — channel closure (client disconnect)
JadepawError::agent_terminated(
    AgentTerminationReason::InfrastructureError {
        reason: format!("client disconnected: {}", err_msg),
        turn,
    },
)

// lib.rs:79-84 — pool acquisition
JadepawError::agent_terminated(
    AgentTerminationReason::InfrastructureError {
        reason: format!("failed to acquire session: {}", e),
        turn: 0,
    },
)

// lib.rs:122-128 — no Finished step
JadepawError::agent_terminated(
    AgentTerminationReason::InfrastructureError {
        reason: "agent completed without producing a final answer".to_string(),
        turn: 0,
    },
)
```

Alternately, if the caller really does need to collate these with Wasm traps, provide a discriminator method on `AgentTerminationReason` that callers can use. But the variant name `WasmTrap` must not be misleading about what actually failed.

### CR-02: Wall-clock timeout truncates sub-second durations to zero

**File:** `crates/jadepaw-agent/src/guard.rs:104-109`
**Issue:**
When the wall-clock timeout fires, both `elapsed_secs` and `max_secs` are populated via `config.wall_clock_timeout.as_secs()`:

```rust
_ = tokio::time::sleep(config.wall_clock_timeout) => {
    Err(JadepawError::agent_terminated(
        AgentTerminationReason::WallClockTimeout {
            elapsed_secs: config.wall_clock_timeout.as_secs(),
            max_secs: config.wall_clock_timeout.as_secs(),
        },
    ))
}
```

`Duration::as_secs()` truncates to whole seconds. For a 500ms timeout, both fields become `0`. The unit test `run_with_guard_timeout_value_propagated` (termination.rs:66-89) even explicitly asserts this behavior: `assert_eq!(max_secs, 0)` for a 10ms timeout. This is a correctness issue because the consumer of `WallClockTimeout` has no way to know the actual intended timeout value. A production monitoring system would log "agent timed out after 0 seconds (limit: 0 seconds)", which is misleading.

**Fix:** Either round up to avoid zero values, or preserve sub-second precision. Since `AgentTerminationReason` derives `PartialEq`/`Eq` and the comment says `u64` was chosen for that reason, the simplest fix is to round up:

```rust
_ = tokio::time::sleep(config.wall_clock_timeout) => {
    let secs = (config.wall_clock_timeout.as_millis() as u64).div_ceil(1000);
    Err(JadepawError::agent_terminated(
        AgentTerminationReason::WallClockTimeout {
            elapsed_secs: secs,
            max_secs: secs,
        },
    ))
}
```

Alternatively, change `WallClockTimeout` to preserve millisecond precision while retaining `Eq`:

```rust
WallClockTimeout {
    /// Elapsed time in milliseconds.
    elapsed_ms: u64,
    /// Configured maximum in milliseconds.
    max_ms: u64,
},
```

Then at the call site:

```rust
let ms = config.wall_clock_timeout.as_millis() as u64;
Err(JadepawError::agent_terminated(
    AgentTerminationReason::WallClockTimeout {
        elapsed_ms: ms,
        max_ms: ms,
    },
))
```

---

## Warnings

### WR-01: `ContinueThinking` turns omit Thought steps from the execution trace

**File:** `crates/jadepaw-agent/src/loop.rs:96-101,165-173`
**Issue:**
When `parse_next_action` returns `LlmDirective::ContinueThinking`, the thought is sent through the SSE channel (line 96-101, before the match) but never pushed to `trace: Vec<ReActStep>`. Only the assistant message is appended to the LLM message history (lines 167-172). This means the `trace` vector returned by `react_loop()` will be missing all `ReActStep::Thought` entries for turns where the LLM did not produce an ACTION or FINAL ANSWER. The `Act` branch also omits the Thought from trace (only adding Action and Observation at lines 143 and 155).

The SSE stream correctly receives the thought events, but the structured `trace` in `AgentResponse` is incomplete. Downstream consumers that inspect the trace programmatically would see only Action/Observation/Finished entries with no reasoning context.

**Fix:** Push the `thought` to `trace` before entering the match, so all branches benefit:

```rust
// After line 98 (the Thought is already constructed):
let thought = ReActStep::Thought {
    content: full_response.clone(),
};
if tx.send(thought.clone()).await.is_err() {
    anyhow::bail!("output channel closed on turn {}", turn);
}
trace.push(thought);  // <-- ADD THIS LINE

// Then proceed with match
let action = llm::parse_next_action(&full_response);
match action { ... }
```

### WR-02: `parse_next_action` directive search not scoped to post-THOUGHT region

**File:** `crates/jadepaw-agent/src/llm.rs:184-221`
**Issue:**
`parse_next_action` independently searches the entire response text for `FINAL ANSWER:` and `ACTION:` (case-insensitively). Meanwhile, `extract_thought` (line 231) correctly identifies the THOUGHT section boundaries by finding where `FINAL ANSWER:` or `ACTION:` begins after the `THOUGHT:` prefix. But `parse_next_action` does its own `find("FINAL ANSWER:")` and `find("ACTION:")` over the full raw response, ignoring the boundary computed by `extract_thought`.

In practice: if the LLM writes `ACTION:` within its THOUGHT section (e.g., "I should take some action: need to verify credentials first"), the parser will match it as a tool invocation directive at the wrong location. The same applies if `FINAL ANSWER:` appears in the THOUGHT (e.g., "The user wants a final answer: so I will..."). This is a false-positive risk that grows with LLM verbosity.

Additionally, the priority order hardcodes "FINAL ANSWER before ACTION" (line 184 checked before line 195), which would mis-handle a response where ACTION appears before FINAL ANSWER (though in the prompt format ACTION should come first anyway -- this is a latent parsing bug).

**Fix:** Restrict the directive search to the post-THOUGHT region:

```rust
pub fn parse_next_action(response: &str) -> LlmDirective {
    let thought = extract_thought(response).unwrap_or_else(|| response.to_string());
    let response_upper = response.to_uppercase();

    // Find the THOUGHT section end to scope directive searches
    let search_start = response_upper
        .find("THOUGHT:")
        .map(|p| p + "THOUGHT:".len())
        .unwrap_or(0);
    let after_thought = &response[search_start..];
    let after_thought_upper = &response_upper[search_start..];

    // Find the first directive in the post-THOUGHT region
    let fa_pos = after_thought_upper.find("FINAL ANSWER:");
    let act_pos = after_thought_upper.find("ACTION:");

    // Pick whichever comes first
    match (fa_pos, act_pos) {
        (Some(fa), Some(act)) if fa < act => {
            let answer = after_thought[fa + "FINAL ANSWER:".len()..].trim().to_string();
            if !answer.is_empty() {
                return LlmDirective::Finish { thought, answer };
            }
        }
        (_, Some(act)) => {
            let action_str = after_thought[act + "ACTION:".len()..].trim();
            if let Some(paren_pos) = action_str.find('(') { /* ... parse tool(args) */ }
            // ... existing tool parsing logic ...
        }
        (Some(fa), None) => {
            let answer = after_thought[fa + "FINAL ANSWER:".len()..].trim().to_string();
            if !answer.is_empty() {
                return LlmDirective::Finish { thought, answer };
            }
        }
        (None, None) => {}
    }

    LlmDirective::ContinueThinking { thought }
}
```

### WR-03: `run_with_guard` maps unknown loop errors with `turn: 0` when `turn` is recoverable

**File:** `crates/jadepaw-agent/src/guard.rs:64-65,91-99`
**Issue:**
The fallback arm (lines 91-99) maps unknown errors to `WasmTrap { reason: err_msg, turn }` where `turn` defaults to 0 from `extract_turn_from_error`. However, many loop errors encode the turn number in their message ("LLM call failed on turn 3", "output channel closed on turn 3"). The `extract_turn_from_error` function (lines 120-130) already correctly parses this pattern. This is not a correctness issue per se, but the hardcoded `turn: 0` comment on line 66 is misleading since `extract_turn_from_error` handles this correctly.

**Fix:** The current code actually works correctly (extract_turn_from_error parses "on turn N"). The doc comment is just stale. Update the comment to reflect reality:

```rust
/// Attempt to extract a turn number from a loop error message.
///
/// The loop uses `anyhow` error messages containing "on turn N". This
/// function parses that pattern and returns the turn number, defaulting
/// to 0 if the turn cannot be determined.
```

(This is already the current doc comment -- the issue is actually NOT a bug, just noting that the previous review version flagged this. The implementation is correct.)

Actually, re-reading the code: `extract_turn_from_error` is only called on line 65, and its result `turn` is used for LLM failures (line 69-73), channel closures (line 86-88), and the fallback (line 94-97). This is correct behavior. The only concern is that `turn: 0` could be ambiguous when extraction fails -- it's indistinguishable from a real turn-0 error. Consider returning `Option<u32>` and using `None` when extraction fails, but this is minor.

### WR-04: Test `run_agent_returns_structured_response` does not test `run_agent`

**File:** `crates/jadepaw-agent/tests/agent_loop.rs:57-68`
**Issue:**
The test creates an `Arc<InstancePool>` and binds it to `_pool`, but never calls `run_agent()`. The comment on line 65 says "skip full integration test without API key; structural verification is handled in the SSE streaming tests." This test passes whether `run_agent` compiles or not. If the function signature changed (e.g., a new required parameter was added), this test would not catch it. It risks giving a false sense of coverage.

**Fix:** Either:
1. Remove the dead test entirely (it asserts nothing), or
2. Write a compile-time type assertion that exercises the function signature:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn run_agent_signature_type_checks() {
    // Verify run_agent's type signature compiles by creating an invalid
    // LLM client that will fail immediately (no API key needed).
    let pool = Arc::new(make_test_pool());
    let config = async_openai::config::OpenAIConfig::new()
        .with_api_base("http://[::1]:1"); // invalid port, immediate fail
    let client = async_openai::Client::with_config(config);
    let result = jadepaw_agent::run_agent(
        jadepaw_core::AgentRequest::default(),
        pool,
        client,
        "gpt-4",
    )
    .await;
    // Expected to fail (connection refused), but type must compile
    assert!(result.is_err());
}
```

---

## Info

### IN-01: `LlmDirective::Finish.thought` and `ContinueThinking.thought` destructured as `_`

**File:** `crates/jadepaw-agent/src/loop.rs:107,165`
**Issue:**
The `thought` field from `LlmDirective::Finish` and `ContinueThinking` is bound with `thought: _`, discarding the LLM's reasoning text. While the full response text is already captured as a `ReActStep::Thought` emitted via the channel before the match, the destructure pattern `thought: _` signals that the field exists but is intentionally unused. This is a code clarity issue: readers might wonder why a field is carried through the type system but never read.

**Fix:** Either remove the `thought` field from the `LlmDirective::Finish` and `ContinueThinking` variants (since the full response text is already used), or explicitly name the unused binding with a comment:

```rust
LlmDirective::Finish { thought: _final_thought, answer } => {
    // _final_thought is available if we decide to expose it in the trace;
    // currently the full response text (emitted as Thought above) suffices.
    // ...
}
```

### IN-02: `core::result::Result` vs `std::result::Result` inconsistency in `lib.rs`

**File:** `crates/jadepaw-agent/src/lib.rs:65`
**Issue:**
The `run_agent` return type uses `core::result::Result<..>` with nested `core::result::Result<Event, Infallible>` inside the `Stream` item type. Meanwhile `jadepaw_core::Result` is an alias for `std::result::Result<T, JadepawError>`. In Rust, `core::result::Result` is a re-export of `std::result::Result` (they are the same type), so this is harmless at runtime, but the inconsistency is confusing to readers. Using `Result` directly (imported from `std::result` or `core::result`) vs the crate alias requires mental context-switching.

**Fix:** Import `std::result::Result` or just use `Result` consistently. For the `Infallible` error type in the stream, a type alias would clarify intent:

```rust
use std::result::Result as StdResult;
use std::convert::Infallible;

pub async fn run_agent(
    // ...
) -> jadepaw_core::Result<(
    AgentResponse,
    impl Stream<Item = StdResult<Event, Infallible>>,
)>
```

---

_Reviewed: 2026-06-01T23:45:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_