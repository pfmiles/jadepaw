---
phase: 03-agent-runtime
fixed_at: 2026-06-02T00:00:00Z
review_path: .planning/phases/03-agent-runtime/03-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 4
skipped: 0
status: all_fixed
---

# Phase 03: Code Review Fix Report

**Fixed at:** 2026-06-02T00:00:00Z
**Source review:** .planning/phases/03-agent-runtime/03-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4
- Fixed: 4
- Skipped: 0

## Fixed Issues

### CR-01: Unbounded async closure captures live mpsc receiver under early-return path

**Files modified:** `crates/jadepaw-agent/src/lib.rs`
**Commit:** 3931671
**Applied fix:** Separated `drop(tx)` from `?` propagation so the mpsc sender is always dropped before any error propagation. The SSE channel close signal is now guaranteed to be sent regardless of whether `run_with_guard` returns an error, preventing downstream SSE consumers from hanging indefinitely.

### WR-01: `extract_turn_from_error` returns 0 for both "error on turn 0" and "could not determine turn"

**Files modified:** `crates/jadepaw-agent/src/guard.rs`
**Commit:** fc840d0
**Applied fix:** Changed `extract_turn_from_error` return type from `u32` to `Option<u32>`. Returns `Some(n)` when a turn is successfully parsed, `None` when the turn cannot be determined. The caller uses `unwrap_or(0)` to preserve existing fallback semantics while making the ambiguity visible at the type level.

### WR-02: `tx.send(finished)` before `trace.push(finished)` creates trace inconsistency window

**Files modified:** `crates/jadepaw-agent/src/loop.rs`
**Commit:** bf97f86
**Applied fix:** Reordered the Finish branch so `trace.push(finished)` occurs before `tx.send(finished)`. Local state is now always updated before external notification goes out, ensuring the upstream caller always finds the Finished step in the trace.

### WR-03: `LlmDirective::Finish { thought: _ }` discards the final thought field

**Files modified:** `crates/jadepaw-agent/src/loop.rs`
**Commit:** 44c37d2
**Applied fix:** Added an explicit comment documenting why the `thought` field from `LlmDirective::Finish` is intentionally discarded -- the reasoning context is already present in the trace via the `ReActStep::Thought` pushed at turn start. The comment prevents future readers from mistaking the silent discard for a data loss bug.

---

_Fixed: 2026-06-02T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_