---
phase: 03-agent-runtime
reviewed: 2026-06-02T00:00:00Z
depth: standard
files_reviewed: 11
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
  - crates/jadepaw-core/tests/agent_types.rs
findings:
  critical: 1
  warning: 3
  info: 1
  total: 5
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-06-02
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Summary

Review of the agent runtime implementation covering the ReAct loop orchestrator, termination guards, LLM integration with async-openai, SSE event relay, core data types, and error handling. The codebase demonstrates well-structured separation of concerns, clear design documentation in module headers, and good use of Rust's type system for error classification.

Key concerns include a potential SSE stream leak when termination guards fire, subtle `extract_turn_from_error` ambiguity in turn-0 cases, a redundant trace push path in the `Finished` branch, and a `Finish` destructure where the thought field is discarded when it could be useful.

## Critical Issues

### CR-01: Unbounded async closure captures live mpsc receiver under early-return path

**File:** `crates/jadepaw-agent/src/lib.rs:94-109`
**Issue:** The closure `|| { r#loop::react_loop(...) }` passed to `run_with_guard` on line 94 captures `tx` by reference. The `tx` is dropped on line 109, *after* `run_with_guard` completes. However, if `run_with_guard` returns `Err` (wall-clock timeout, iteration limit, loop error), the `?` operator on line 106 propagates the error immediately, skipping line 109 entirely. The `tx` remains alive, and the returned `sse_stream` (constructed on line 88) wraps a `ReceiverStream` whose receiver will never receive a close signal. Any downstream consumer polling `sse_stream` will block indefinitely waiting for the next event that never arrives.

This is a resource leak in the streaming pipeline: the SSE stream never terminates naturally, and the consumer (axum response, HTMX frontend) hangs.

**Fix:** Drop `tx` *before* propagating the error:
```rust
let trace = guard::run_with_guard(&guard_config, || {
    r#loop::react_loop(
        &guard_config,
        &mut handle,
        &llm_client,
        model,
        system_prompt,
        &user_message,
        context,
        &tx,
    )
})
.await;
drop(tx); // always executed, regardless of result

let trace = trace?; // now propagate error after dropping tx

// Extract final answer (line 114-128) continues as before...
```

Alternatively, use a `Drop` guard or defer mechanism to ensure `tx` is always dropped before the function returns.

## Warnings

### WR-01: `extract_turn_from_error` returns 0 for both "error on turn 0" and "could not determine turn"

**File:** `crates/jadepaw-agent/src/guard.rs:124-133`
**Issue:** The fallback path in `extract_turn_from_error` returns `0` when the turn number cannot be parsed from the error message. This makes the return value ambiguous: both a legitimate error on turn 0 and a parsing failure produce the same `turn: 0` output. The comment on lines 121-123 acknowledges this ambiguity but does not provide a mechanism for callers to distinguish the two cases. The caller at lines 95-96 always uses the returned value directly, potentially mis-attributing an error to turn 0 when the turn was actually unparseable.

**Fix:** Return `Option<u32>` from `extract_turn_from_error` and let the caller decide the fallback:
```rust
fn extract_turn_from_error(err_msg: &str) -> Option<u32> {
    if let Some(turn_pos) = err_msg.find("on turn ") {
        let after = &err_msg[turn_pos + "on turn ".len()..];
        let turn_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(turn) = turn_str.parse::<u32>() {
            return Some(turn);
        }
    }
    None
}
```
In the caller:
```rust
let turn = extract_turn_from_error(&err_msg).unwrap_or(0);
```

### WR-02: `tx.send(finished)` before `trace.push(finished)` creates trace inconsistency window

**File:** `crates/jadepaw-agent/src/loop.rs:164-175`
**Issue:** When the LLM directive is `LlmDirective::Finish`, the `finished` step is sent through `tx` (SSE channel) at line 170 *before* it is pushed to the local `trace` at line 173. If the SSE consumer processes the `done` event, disconnects, and the channel close triggers immediately, there is a narrow window where `trace.push(finished)` at line 173 may not execute before the function returns. The upstream `run_agent` function (lines 114-128) would then fail with "agent completed without producing a final answer" because it won't find the `Finished` step in the trace.

While this is an unlikely race condition (the `Finished` branch returns `Ok(trace)` synchronously after the push), the ordering is still semantically wrong -- the local state should be updated before external notification.

**Fix:** Push to `trace` before sending to `tx`:
```rust
LlmDirective::Finish { thought: _, answer } => {
    let finished = ReActStep::Finished {
        answer: answer.clone(),
    };
    trace.push(finished.clone());
    if tx.send(finished).await.is_err() {
        return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
    }
    return Ok(trace);
}
```

### WR-03: `LlmDirective::Finish { thought: _ }` discards the final thought field

**File:** `crates/jadepaw-agent/src/loop.rs:165`
**Issue:** The `LlmDirective::Finish` variant carries a `thought` field (the LLM's final reasoning before the answer), but this field is destructured as `_` (discarded) on line 165. The `Finished` `ReActStep` only contains the `answer`. The reasoning content is indirectly preserved via the `ReActStep::Thought` pushed to trace on line 159, so this is not a data loss bug today. However, this is a fragile dependency: if the `Thought` event is ever conditionally emitted or reordered, the reasoning context for the final answer would be lost silently.

**Fix:** Add a comment explicitly documenting the intentional coupling:
```rust
LlmDirective::Finish { thought: final_thought, answer } => {
    // final_thought is intentionally NOT stored in Finished step --
    // it is already present in the trace from the ReActStep::Thought
    // pushed at the start of this turn (line 159).
    let finished = ReActStep::Finished {
        answer: answer.clone(),
    };
    // ...
}
```
Or consider adding an optional `thought` field to `ReActStep::Finished` for completeness.

## Info

### IN-01: `temp_dir()` usage in `run_agent` creates non-deterministic sandbox roots

**File:** `crates/jadepaw-agent/src/lib.rs:72`
**Issue:** The `run_agent` function uses `std::env::temp_dir()` to create a sandbox root for `SessionState::with_defaults()`. This picks up the system temp directory, which varies between environments and platforms. In multi-tenant deployment scenarios, this could lead to sandbox isolation issues or conflicts between concurrent sessions using the same temp directory subtree.

**Fix:** Accept a configurable sandbox root as a parameter, or use a dedicated directory structure:
```rust
pub async fn run_agent(
    req: AgentRequest,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    sandbox_root: PathBuf,  // new parameter
) -> ... {
    let state = SessionState::with_defaults(sandbox_root);
    // ...
}
```

---

_Reviewed: 2026-06-02T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_