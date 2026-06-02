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
  critical: 1
  warning: 0
  info: 3
  total: 4
status: issues_found
---

# Phase 03: Code Review Report (Round 3)

**Reviewed:** 2026-06-03T00:00:00Z
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Summary

Third re-review of Phase 03 agent-runtime. All six previously identified issues (CR-01, WR-01 through WR-06 from rounds 1-2) have been verified as correctly fixed and are confirmed resolved:

| Previous ID | Status | Verification |
|---|---|---|
| CR-01 (SSE stream leak) | Fixed | `drop(tx)` at lib.rs:112 precedes error propagation |
| WR-01 (turn extraction ambiguity) | Fixed | `extract_turn_from_error` returns `Option<u32>` |
| WR-02 (Finished trace ordering) | Fixed | `trace.push()` before `tx.send()` in Finish branch (loop.rs:181-184) |
| WR-03 (discarded thought field) | Fixed | Documentation block at loop.rs:167-172 |
| WR-04 (Act branch send-then-push) | Fixed | `trace.push()` before `tx.send()` in Act branch (loop.rs:211-213) |
| WR-05 (fragile string parsing) | Fixed | Structured `|turn=N|` marker (guard.rs:128, loop.rs:142) |
| WR-06 (misleading tx parameter) | Fixed | Renamed to `close_signal` (llm.rs:127) |

All 34 tests pass across jadepaw-agent, jadepaw-core, and jadepaw-wasm. The project compiles cleanly with zero warnings.

One **new critical issue** was identified: the fuel-reset error path in the ReAct loop bypasses the `LoopErrorKind` structured classification system, causing host-side infrastructure errors to be misclassified as wasm guest traps.

Three info-level findings from the previous review (IN-01: temp_dir, IN-02: hardcoded GuardConfig, IN-03: elapsed_ms field semantics) remain unfixed but are documented below for completeness.

## Critical Issues

### CR-01: Fuel reset failure misclassified as WasmTrap instead of InfrastructureError

**File:** `crates/jadepaw-agent/src/loop.rs:127-132`
**Issue:** The per-turn fuel reset on the wasmtime `Store` uses `anyhow::anyhow!()` directly with the `?` operator, producing a plain `anyhow::Error` that is NOT wrapped in `LoopErrorKind`. This is the only error path in `react_loop()` that bypasses the structured error classification system.

Error propagation chain:
1. `loop.rs:130-132` — `session.store_mut().set_fuel(1_000_000)` fails, error wrapped as `anyhow::anyhow!("failed to set fuel on session store: {}", e)`, propagated via `?`
2. `react_loop` returns `Err(plain_anyhow_error)` to `run_with_guard`
3. `guard.rs:64` — `e.downcast_ref::<LoopErrorKind>()` returns `None` because the error was created with `anyhow::anyhow!()`, not `loop_error()`
4. `guard.rs:92-102` — fallback branch executes: `extract_turn_from_error` returns `None` (message lacks `|turn=N|` marker), turn defaults to 0, error classified as `AgentTerminationReason::WasmTrap`

The resulting `WasmTrap` classification is semantically wrong: a fuel reset failure is a host-side infrastructure/config issue, not a guest sandbox violation. Downstream consumers inspecting `AgentTerminationReason` would misinterpret this as a guest module bug rather than a host configuration problem.

Every other error path in `react_loop` correctly routes through `LoopErrorKind`:
- LLM failures: `LoopErrorKind::LlmFailure` (line 144)
- Channel closures: `LoopErrorKind::ChannelClosed` (lines 155, 183, 213, 225)
- Max iterations: `LoopErrorKind::MaxIterations` (line 248)

**Fix:**
```rust
// In crates/jadepaw-agent/src/loop.rs, lines 127-132, change from:
        session
            .store_mut()
            .set_fuel(1_000_000)
            .map_err(|e| {
                anyhow::anyhow!("failed to set fuel on session store: {}", e)
            })?;

// To:
        session
            .store_mut()
            .set_fuel(1_000_000)
            .map_err(|e| {
                loop_error(LoopErrorKind::LlmFailure {
                    turn,
                    source: anyhow::anyhow!("failed to set fuel on session store: {}", e),
                })
            })?;
```

This wraps the error in `LoopErrorKind::LlmFailure` with the correct turn number, enabling `run_with_guard`'s downcast path (guard.rs:64-73) to classify it as `AgentTerminationReason::InfrastructureError`. The `LlmFailure` variant name is slightly imprecise here (the failure is store/engine, not LLM), but its guard mapping to `InfrastructureError` is correct. For maximum precision, a dedicated variant like `LoopErrorKind::StoreError { turn, source }` could be introduced and mapped identically.

## Info

### IN-01: `temp_dir()` usage persists — no configurable sandbox root

**File:** `crates/jadepaw-agent/src/lib.rs:72`
**Issue:** (Carried from previous review.) `run_agent` uses `std::env::temp_dir()` for the sandbox root. In multi-tenant deployments, concurrent sessions using the same temp directory subtree could face isolation boundary issues. The sandbox root should be a configurable parameter rather than hardcoded to the OS temp directory.

**Suggested fix:** Accept `sandbox_root: PathBuf` as a parameter to `run_agent`, or derive from a tenant-specific config.

### IN-02: Hardcoded `GuardConfig::default()` prevents per-skill/per-tenant customization

**File:** `crates/jadepaw-agent/src/lib.rs:90`
**Issue:** (Carried from previous review.) `run_agent` always uses `GuardConfig::default()` (20 iterations, 300s timeout). Different skills may warrant different limits. Without parameterization, all sessions share identical constraints.

**Suggested fix:** Accept `guard_config: Option<GuardConfig>` in `run_agent`'s signature, falling back to `Default::default()` when `None`.

### IN-03: `WallClockTimeout.elapsed_ms` always equals `max_ms` in current code

**File:** `crates/jadepaw-agent/src/guard.rs:106-114`
**Issue:** (Carried from previous review.) When the wall-clock timeout fires, line 107 sets `elapsed_ms` to `config.wall_clock_timeout.as_millis() as u64`, which is identical to `max_ms`. The field name implies actual elapsed time, but the value reported is always the configured limit. This reduces debuggability — a consumer cannot distinguish "timed out at exactly the limit" from "timeout value is reported as elapsed."

**Suggested fix:** Add a doc comment on the `WallClockTimeout` variant clarifying that `elapsed_ms` is approximate and equals `max_ms` when the timeout fires from `tokio::select!`.

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_