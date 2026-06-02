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
  warning: 0
  info: 3
  total: 3
status: issues_found
---

# Phase 03: Code Review Report (Round 4 — Final Re-review)

**Reviewed:** 2026-06-03T00:00:00Z
**Depth:** standard
**Files Reviewed:** 11
**Status:** issues_found

## Summary

Fourth and final re-review of Phase 03 agent-runtime after all 8 prior fixes across 3 rounds. All previously identified issues (CR-01 from rounds 1 and 3, WR-01 through WR-06) have been verified as correctly fixed:

| Previous ID | Status | Verification |
|---|---|---|
| CR-01 round 1 (SSE stream leak) | Fixed | `drop(tx)` at lib.rs:112 precedes error propagation |
| CR-01 round 3 (fuel reset misclassification) | Fixed | Commit `ea7c7d8` — fuel reset uses `loop_error(LoopErrorKind::LlmFailure {...})` at loop.rs:131 |
| WR-01 (turn extraction ambiguity) | Fixed | `extract_turn_from_error` returns `Option<u32>` |
| WR-02 (Finished trace ordering) | Fixed | `trace.push()` before `tx.send()` in Finish branch (loop.rs:181-184) |
| WR-03 (discarded thought field) | Fixed | Documentation block at loop.rs:167-172 |
| WR-04 (Act branch send-then-push) | Fixed | `trace.push()` before `tx.send()` in Act branch (loop.rs:211-213) |
| WR-05 (fragile string parsing) | Fixed | Structured `|turn=N|` marker (guard.rs:128, loop.rs:142) |
| WR-06 (misleading tx parameter) | Fixed | Renamed to `close_signal` (llm.rs:127) |

**Verification methodology:**
- Full cargo check of all targets — zero warnings
- Full test suite — 80 tests passed, 1 ignored (stress test), 0 failed
- Manual line-by-line trace of all error propagation paths in guard.rs and loop.rs
- Cross-crate type verification for jadepaw-core, jadepaw-agent, and jadepaw-wasm boundaries
- Security scan for hardcoded secrets, unsafe blocks, dangerous function calls — all clean

**No new critical or warning-level issues found.** The three info-level findings from previous rounds (IN-01: temp_dir, IN-02: hardcoded GuardConfig, IN-03: elapsed_ms field semantics) remain as documented below for completeness. These are not new findings and are carried forward for tracking purposes.

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
**Issue:** (Carried from previous review.) When the wall-clock timeout fires via `tokio::select!`, `elapsed_ms` is set to `config.wall_clock_timeout.as_millis() as u64`, which is identical to `max_ms`. The field name implies actual elapsed time, but the value reported is always the configured limit. This reduces debuggability — a consumer cannot distinguish "timed out at exactly the limit" from "timeout value is reported as elapsed."

**Suggested fix:** Add a doc comment on the `WallClockTimeout` variant clarifying that `elapsed_ms` is approximate and equals `max_ms` when the timeout fires from `tokio::select!`.

---

_Reviewed: 2026-06-03T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_