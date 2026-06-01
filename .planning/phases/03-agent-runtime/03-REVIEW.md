---
phase: 03-agent-runtime
reviewed: 2026-06-02T12:00:00Z
depth: standard
files_reviewed: 16
files_reviewed_list:
  - Cargo.toml
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-agent/tests/termination.rs
  - crates/jadepaw-core/Cargo.toml
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/error.rs
  - crates/jadepaw-core/src/guest_exports.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/tests/agent_types.rs
findings:
  critical: 0
  warning: 3
  info: 3
  total: 6
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-06-02T12:00:00Z
**Depth:** standard
**Files Reviewed:** 16
**Status:** issues_found

## Summary

Re-reviewed the Phase 03 agent runtime implementation after prior fix iteration. All four previously-reported warnings (WR-01 through WR-04 from review iteration 1) have been correctly addressed: unused dependencies removed, `GuardConfig` is now borrowed, `TenantQuotaLimiter` carries a deferred-wiring doc comment, and the `new_with_budget` doc has been corrected.

No critical (blocker) issues were found. Three new warnings are identified: a configuration divergence risk between `LoopConfig.max_iterations` and `GuardConfig.max_iterations`, silent serialization failure in SSE event production via `.unwrap_or_default()`, and fragile string-based error classification in the termination guard. Three informational items cover unreachable dead code, redundant configuration, and clock-start semantics.

---

## Warnings

### WR-01: `LoopConfig.max_iterations` and `GuardConfig.max_iterations` are independent sources of truth enabling divergence

**File:** `crates/jadepaw-agent/src/lib.rs:90-91`, `crates/jadepaw-agent/src/guard.rs:24,71-72`, `crates/jadepaw-agent/src/loop.rs:28,76,182`
**Issue:** `run_agent()` constructs `LoopConfig::default()` and `GuardConfig::default()` independently (lib.rs:90-91), both with `max_iterations: 20`. The `LoopConfig.max_iterations` controls the actual loop termination (`for turn in 0..config.max_iterations` at loop.rs:76), while `GuardConfig.max_iterations` is only used to populate the `max` field in the `MaxIterationsReached` error variant (guard.rs:71). There is no mechanism to ensure these two values stay synchronized. If a caller changes only one — for example, setting `LoopConfig { max_iterations: 10 }` while leaving `GuardConfig::default()` — the loop would actually stop at turn 10, but the error report would claim `max: 20`. Conversely, setting `GuardConfig { max_iterations: 10, .. }` with `LoopConfig::default()` would report `max: 10` in errors while the loop runs for 20 turns.

**Fix:** There are two reasonable approaches:

Option A — Eliminate `LoopConfig` and have `react_loop` use `GuardConfig.max_iterations` directly:
```rust
// lib.rs: remove LoopConfig construction, pass guard_config to react_loop
pub async fn run_agent(...) -> ... {
    let guard_config = guard::GuardConfig::default();
    // ...
    let trace = guard::run_with_guard(&guard_config, || {
        r#loop::react_loop(
            &guard_config,   // GuardConfig carries max_iterations
            &mut handle,
            // ...
        )
    }).await?;
}
```

Option B — Have `GuardConfig` reference `LoopConfig` and delegate:
```rust
// guard.rs
impl GuardConfig {
    pub fn from_loop_config(loop_config: &LoopConfig, wall_clock_timeout: Duration) -> Self {
        Self { max_iterations: loop_config.max_iterations, wall_clock_timeout }
    }
}
```

Option A is simpler since `LoopConfig` currently only carries `max_iterations` (making it a redundant wrapper). If future loop configuration fields are planned, Option B provides better separation of concerns.

### WR-02: `serde_json::to_string` with `.unwrap_or_default()` silently swallows serialization failures in SSE event production

**File:** `crates/jadepaw-agent/src/stream.rs:59-63, 69-74, 77-82`
**Issue:** Three SSE event constructors map `serde_json::to_string(...).unwrap_or_default()` to produce event data. If `serde_json::to_string` returns `Err` — which can happen for non-finite float values (`NaN`, `Infinity`), deeply nested structures exceeding `serde_json` depth limits, or custom serializer failures — the result is an empty `String`, producing an SSE event with empty `data:` payload. The caller (SSE consumer) receives a valid-looking event that silently carries no information, rather than being notified of the failure. The `ReActStep::Action.args` field is `serde_json::Value`, which in theory can carry arbitrary JSON including non-finite numbers; if such a value enters the system (e.g., from an LLM that returns a tool argument with `NaN`), the error would be silently swallowed.

**Fix:** Emit a JSON error payload on serialization failure so the SSE consumer is aware something went wrong:
```rust
// stream.rs, in the Action mapping arm:
ReActStep::Action { tool, args } => {
    let payload = serde_json::to_string(&serde_json::json!({
        "tool": tool,
        "args": args,
    }))
    .unwrap_or_else(|e| {
        serde_json::json!({
            "tool": tool,
            "error": "failed to serialize tool args",
            "serialization_error": e.to_string(),
        })
        .to_string()
    });
    Event::default().event("action").data(payload)
}
```

Alternatively, since `serde_json::Value` serialization should be infallible under normal circumstances, wrap in a defensive `debug_assert!` that fails in tests but logs a warning in production:
```rust
let payload = match serde_json::to_string(&serde_json::json!({...})) {
    Ok(s) => s,
    Err(e) => {
        tracing::error!("failed to serialize SSE event data: {e}");
        String::new()
    }
};
```

### WR-03: Error classification in `run_with_guard` uses fragile string matching on `anyhow` error messages that may contain arbitrary downstream text

**File:** `crates/jadepaw-agent/src/guard.rs:57-103`
**Issue:** The error classification logic in `run_with_guard` calls `e.to_string()` (line 58) and then uses `.contains()` to check for substrings: `"max iterations"` (line 67), `"LLM call failed"` (line 74), and `"output channel closed"` (line 84). The `e` is an `anyhow::Error`, and `to_string()` renders the full error chain. The LLM error path (loop.rs:93) wraps downstream errors with `with_context(|| format!("LLM call failed on turn {}", turn))`. The resulting display includes the context chain: `"LLM call failed on turn 3: <original error>"`. If the downstream error (from async-openai, the HTTP client, or the remote API) happens to contain the substring `"max iterations"` — plausible for API rate-limit errors like `"Rate limit exceeded: max iterations per minute reached"` — the guard would misclassify an LLM/infrastructure error as `MaxIterationsReached`. Similarly, a provider error message containing `"LLM call failed"` or `"output channel closed"` (from nested contexts) could cause misclassification between `InfrastructureError` and `WasmTrap`.

**Fix:** Use structured error types instead of string matching. A minimal change is to use `anyhow`'s `.downcast_ref::<ErrorType>()` or chain inspection. A more robust approach is to define an internal error enum and map it explicitly:

```rust
// New internal error type in loop.rs or a shared location
#[derive(Debug)]
enum LoopErrorKind {
    MaxIterations { iter: u32, max: u32 },
    LlmFailure { turn: u32, source: anyhow::Error },
    ChannelClosed { turn: u32 },
}

// In react_loop, wrap errors with a known type
anyhow::bail!(LoopErrorKind::MaxIterations { iter: *turn, max: config.max_iterations });

// In guard.rs, downcast to inspect
match e.downcast_ref::<LoopErrorKind>() {
    Some(LoopErrorKind::MaxIterations { max, .. }) => { ... }
    Some(LoopErrorKind::LlmFailure { turn, .. }) => { ... }
    Some(LoopErrorKind::ChannelClosed { .. }) => { ... }
    None => { /* unknown error -> WasmTrap */ }
}
```

## Info

### IN-01: Dead error-path code in `run_agent` final answer extraction

**File:** `crates/jadepaw-agent/src/lib.rs:115-129`
**Issue:** The `.ok_or_else()` fallback on `final_answer` extraction (lines 121-129) creates an `InfrastructureError` with the message "agent completed without producing a final answer". This code path is unreachable. Every execution path through `react_loop` either returns `Ok(trace)` containing at least one `ReActStep::Finished` step, or returns `Err`. Since `run_with_guard` propagates the `Err` via the `?` operator on line 107, control only reaches line 115 when the guard call returned `Ok(trace)`. In that case, `trace` always contains a `Finished` step (it is inserted by `react_loop` at loop.rs:112-118 on the `LlmDirective::Finish` branch before `return Ok(trace)`). The fallback is dead code and creates a false impression that the agent can complete without a final answer.

**Fix:** Either replace `.ok_or_else()` with `.expect("react_loop invariant: trace must contain a Finished step on Ok return")` to document the invariant, or remove the fallback entirely if the type system can encode the guarantee.

### IN-02: `GuardConfig.max_iterations` is redundant — never enforced by the guard, only used in error display

**File:** `crates/jadepaw-agent/src/guard.rs:24,71-72`
**Issue:** The `GuardConfig.max_iterations` field is declared and stored but the `run_with_guard` function never compares any iteration count against it. The guard only races against wall-clock timeout. The iteration limit is enforced entirely inside `react_loop` via `LoopConfig.max_iterations` (loop.rs:76). `GuardConfig.max_iterations` is only read on line 71 to populate the `max` field in `AgentTerminationReason::MaxIterationsReached`. This makes `GuardConfig` carry a value it does not use for enforcement, creating a misleading API surface where callers might expect `GuardConfig` to enforce the limit.

**Fix:** See WR-01 fix above. The clean resolution is to make `GuardConfig` not carry a `max_iterations` field at all, and have the loop report its own max value in the error. If the error classification used structured types (see WR-03 fix), the loop could directly set the `max` field from `LoopConfig.max_iterations` without involving `GuardConfig`.

### IN-03: Wall-clock timeout clock starts before closure execution, consuming setup time from the agent budget

**File:** `crates/jadepaw-agent/src/guard.rs:55,107`
**Issue:** `tokio::select!` starts the `sleep(config.wall_clock_timeout)` timer at the moment `run_with_guard` is called (line 55), which is the same moment the closure `agent_loop` is invoked. However, `agent_loop` calls `react_loop` which calls `session.store_mut().set_fuel()` and `llm::stream_llm_response()` only after invocation. Any time spent in closure execution setup, session acquisition, or other pre-loop operations burns into the wall-clock budget. The design doc (D-08) explicitly states "Wall-clock timeout cannot be reset or extended by any code path", which means this behavior is intentional. However, in the current `run_agent` implementation (lib.rs:73-85), session acquisition happens *before* `run_with_guard` is called, so the guard only covers the loop execution itself. If a future caller interleaves setup work between the closure start and the actual loop body, the timer would be counting against that work.

**Fix:** No code change needed — the behavior matches the documented design. Consider adding a brief comment noting the timer semantics:
```rust
/// The wall-clock timeout starts when `run_with_guard` is called, not when
/// the inner loop body begins. Callers should complete session acquisition
/// and setup before invoking this function.
```

---

_Reviewed: 2026-06-02T12:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_