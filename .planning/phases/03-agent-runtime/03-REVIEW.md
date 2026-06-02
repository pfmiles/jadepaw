---
phase: 03-agent-runtime
reviewed: 2026-06-03T00:00:00Z
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
  critical: 0
  warning: 3
  info: 3
  total: 6
status: issues_found
---

# Phase 03: Code Review Report (Re-Review)

**Reviewed:** 2026-06-03
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Summary

Re-review of the Phase 03 agent-runtime code after the fix chain for CR-01, WR-01, WR-02, and WR-03. All four fixes have been verified correct:

- **CR-01** (SSE stream leak): `drop(tx)` now precedes error propagation in `run_agent` (lib.rs:112-114). Confirmed fixed.
- **WR-01** (`extract_turn_from_error` ambiguity): Function now returns `Option<u32>` with caller using `.unwrap_or(0)` (guard.rs:126-135, 95). Confirmed fixed.
- **WR-02** (Finished trace ordering): `trace.push(finished)` now happens before `tx.send(finished)` in the Finished branch (loop.rs:181-184). Confirmed fixed.
- **WR-03** (discarded thought field): Detailed comment block documents the intentional coupling between the `LlmDirective::Finish.thought` discard and the `ReActStep::Thought` already in trace (loop.rs:167-172). Confirmed fixed.

All 31 tests pass across jadepaw-agent and jadepaw-core (zero failures).

Three new warnings were identified during re-review: inconsistent send-then-push ordering in the Act branch (not fixed alongside WR-02), fragile string parsing in `extract_turn_from_error`, and a misleading interface on `stream_llm_response`. Three info-level findings are also documented.

## Warnings

### WR-04: Act branch still uses send-then-push (inconsistent with WR-02 fix)

**File:** `crates/jadepaw-agent/src/loop.rs:209-224`
**Issue:** WR-02 fixed the ordering in the `LlmDirective::Finish` branch to push to `trace` before sending to `tx`. However, the `LlmDirective::Act` branch still uses the reverse pattern: `action_step` is sent via `tx` at line 209 before being pushed to `trace` at line 212. The observation step follows the same pattern: sent at line 221, pushed at line 224.

The consequences differ between the two branches:
- **Finish**: Returns `Ok(trace)` on success. Without WR-02's fix, a channel close during send would leave the Finished step missing from the trace, causing `run_agent` to fail with "agent completed without producing a final answer".
- **Act**: Returns `Err(ChannelClosed)` on channel close, so the caller never sees the incomplete trace. The ordering inconsistency is defensible but creates a maintenance hazard -- future developers expecting the WR-02 pattern may accidentally modify one branch without updating the other, or refactor the error handling in the Act branch without realizing the trace could be incomplete.

**Fix:** Apply the same push-before-send pattern to the Act branch for consistency and defensive correctness:
```rust
// Push to trace before sending to tx
trace.push(action_step.clone());
if tx.send(action_step).await.is_err() {
    return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
}
// Same for observation
trace.push(observation.clone());
if tx.send(observation).await.is_err() {
    return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
}
```

### WR-05: Fragile `extract_turn_from_error` string parsing — first match may be wrong

**File:** `crates/jadepaw-agent/src/guard.rs:126-135`
**Issue:** The `extract_turn_from_error` function searches the full anyhow error chain string for the first occurrence of "on turn ". The loop code at loop.rs:142 adds context via `.with_context(|| format!("LLM call failed on turn {}", turn))`, producing "LLM call failed on turn N" at the front of the Display chain. If the underlying source error (from async-openai, reqwest, or tokio) also happens to contain "on turn " in its error message, the function could extract a turn number from the wrong position.

While the first match is typically the context message (which uses the correct turn number), anyhow's Display order is not guaranteed by contract -- it's an implementation detail. Additionally, if a future code change adds context messages containing "on turn N" at multiple layers, the function would pick the first one arbitrarily.

**Fix:** Use a more specific prefix for the context message that uniquely identifies the intended extraction target:
```rust
// In loop.rs, use a distinctive marker:
.with_context(|| format!("LLM call failed |turn={}|", turn))

// In guard.rs, search for the specific marker:
if let Some(turn_pos) = err_msg.find("|turn=") {
    let after = &err_msg[turn_pos + "|turn=".len()..];
    let turn_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    if let Ok(turn) = turn_str.parse::<u32>() {
        return Some(turn);
    }
}
```
Alternatively, switch to a structured error approach (e.g., a dedicated error type field) to avoid string parsing entirely.

### WR-06: `stream_llm_response` takes `tx: &Sender<ReActStep>` but only uses `is_closed()`

**File:** `crates/jadepaw-agent/src/llm.rs:122-163`
**Issue:** The function signature at line 123-127 accepts `tx: &mpsc::Sender<ReActStep>` but uses it solely for the `tx.is_closed()` check at line 152 to bail out early. The parameter name and type signal that the function *can* send events, but it does not. This is a misleading interface: a caller reading the signature would assume per-token SSE events are emitted here, but the actual emit happens in the caller (`react_loop` at loop.rs:154).

The documentation at lines 112-114 clarifies this, but interface design should be self-documenting where possible -- a parameter that carries incorrect implications is worse than no parameter.

**Fix:** Accept a simpler signal for the closed check, or rename the parameter to reflect its actual role:
```rust
pub async fn stream_llm_response(
    client: &Client<Box<dyn Config>>,
    messages: Vec<ChatCompletionRequestMessage>,
    model: &str,
    close_signal: &mpsc::Sender<ReActStep>,  // renamed: clarifies it's only for signal
) -> anyhow::Result<String> {
```

Alternatively, accept a `tokio::sync::watch::Receiver<bool>` or a simple `&AtomicBool` for the stop signal, separating concerns cleanly.

## Info

### IN-01: `temp_dir()` usage persists (carried from previous review IN-01)

**File:** `crates/jadepaw-agent/src/lib.rs:72`
**Issue:** The `run_agent` function calls `std::env::temp_dir()` to create a sandbox root for `SessionState::with_defaults()`. This uses the system temporary directory, which varies between environments, platforms, and OS users. In multi-tenant deployments, concurrent sessions using the same temp directory subtree could encounter isolation boundary issues.

**Fix:** Accept a configurable sandbox root as a parameter, or construct a dedicated directory structure under a well-known path:
```rust
pub async fn run_agent(
    req: AgentRequest,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    sandbox_root: PathBuf,
) -> ... {
```

### IN-02: Hardcoded `GuardConfig::default()` prevents per-skill customization

**File:** `crates/jadepaw-agent/src/lib.rs:90`
**Issue:** `run_agent` always uses `GuardConfig::default()` (20 iterations, 300-second timeout) with no mechanism for callers to override. Different skills or tenants may warrant different limits -- a simple calculation skill may need 5 iterations, while a complex research skill may need 50. Without parameterization, all sessions share the same constraints.

**Fix:** Accept an optional `GuardConfig` parameter:
```rust
pub async fn run_agent(
    req: AgentRequest,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    guard_config: Option<GuardConfig>,
) -> ... {
    let guard_config = guard_config.unwrap_or_default();
```

### IN-03: `WallClockTimeout.elapsed_ms` is set to `max_ms`, not actual elapsed

**File:** `crates/jadepaw-agent/src/guard.rs:106-114`
**Issue:** When the wall-clock timeout fires, both `elapsed_ms` and `max_ms` are set to `config.wall_clock_timeout.as_millis() as u64` (line 107-111). The field name `elapsed_ms` implies actual time elapsed, but the value is always identical to `max_ms` in this code path. The only time `elapsed_ms` could differ from `max_ms` is if constructed elsewhere manually.

This makes debugging harder: a consumer sees `elapsed_ms: 300000, max_ms: 300000` and cannot distinguish "timed out at exactly 300 seconds" from "timeout was configured for 300 seconds and we report the timeout as happening at the config value."

**Fix:** If actual elapsed measurement is not practical from the `tokio::select!` branch, rename the field or add a comment clarifying the semantics:
```rust
WallClockTimeout {
    /// Approximate elapsed time in milliseconds.
    /// When the timeout fires, this equals the configured max.
    /// A precise elapsed measurement is not available from the select branch.
    elapsed_ms: u64,
```

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_