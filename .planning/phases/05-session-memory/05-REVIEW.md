---
phase: 05-session-memory
reviewed: 2026-06-05T20:00:00Z
depth: standard
files_reviewed: 16
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/window.rs
  - crates/jadepaw-agent/tests/context_window.rs
  - crates/jadepaw-agent/tests/session_persistence.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/types.rs
  - crates/jadepaw-db/Cargo.toml
  - crates/jadepaw-db/migrations/20260604000001_create_sessions.sql
  - crates/jadepaw-db/src/lib.rs
  - crates/jadepaw-db/src/migrations.rs
  - crates/jadepaw-db/src/models.rs
  - crates/jadepaw-db/src/repository.rs
  - crates/jadepaw-db/src/sqlite_repo.rs
findings:
  critical: 1
  warning: 3
  info: 2
  total: 6
status: issues_found
---

# Phase 05: Code Review Report (Adversarial Re-review #4)

**Reviewed:** 2026-06-05
**Depth:** standard
**Files Reviewed:** 16
**Status:** issues_found

## Summary

This is the fourth adversarial re-review of Phase 05 (session-memory). Three previous fix iterations addressed 15 out of 15 Critical+Warning findings. The current code is in good shape — all prior Critical and Warning items have been properly remediated.

This review finds 1 new Critical issue (a silently wrapping integer cast on the DB read path, sibling to the previously-fixed WR-02), 3 Warnings (2 carry-over Info items now upgraded due to real-world impact, plus 1 new config validation gap), and 2 Info items. The overall quality is approaching production-ready.

## Critical Issues

### CR-01: `iteration_count as u32` silently wraps negative values into large positive u32 on DB read path

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:202`
**Issue:** The `load()` function reads `iteration_count` from SQLite as `i32` (line 179: `let iteration_count: i32 = row.get("iteration_count")`) and then unconditionally casts to `u32` at line 202: `iteration_count: iteration_count as u32`. This is the exact same anti-pattern that was previously flagged as WR-02 for `elapsed_ms as u64` at the same function (line 200, now fixed with `try_from`). The previous fix only addressed `elapsed_ms` but overlooked the identical problem with `iteration_count` at the adjacent line.

If a negative value enters the database (via data corruption, manual manipulation, or a future migration bug), the `as u32` cast would silently produce an enormous positive number. For example, `-1_i32 as u32 = 4294967295`. When `resume_session` then uses this as `start_turn`, the agent loop range `start_turn..max_iterations` would be empty (since `start_turn >> max_iterations`), causing the agent to immediately terminate with a `MaxIterationsReached` error — but the error context would report a nonsensical iteration count.

The codebase's own defense-in-depth pattern (applied at lines 200-201 for `elapsed_ms`) should be replicated here.

**Fix:**
```rust
// Line 202, replace:
iteration_count: iteration_count as u32,
// with:
iteration_count: u32::try_from(iteration_count)
    .context("iteration_count is negative or exceeds u32")?,
```

## Warnings

### WR-01: `GuardConfig::validate()` allows `recent_turns = 0`, causing context loss on compression

**File:** `crates/jadepaw-agent/src/guard.rs:57-68`
**Issue:** The `validate()` method only checks `recent_turns > 100` but allows `recent_turns = 0`. When `recent_n = 0` reaches `compress_context`, `n_msgs_to_keep = 0`, meaning zero recent turns are preserved verbatim. The function strips all conversation body messages, keeping only the system prompt, the initial user message, and a summary of older messages. This effectively discards ALL recent conversation context, which can cause the LLM to lose the immediate conversational thread and produce incoherent responses.

The `validate()` function was added in the most recent fix commit specifically to prevent unreasonable config values deserialized from user input. A `recent_turns = 0` value is unreasonable — preserving zero turns makes the context window compression destructive rather than helpful.

**Fix:**
```rust
// In guard.rs validate(), add a lower bound check:
if self.recent_turns == 0 {
    return Err("recent_turns must be at least 1");
}
if self.recent_turns > 100 {
    return Err("recent_turns must not exceed 100");
}
```

### WR-02: `SessionSnapshot.termination_reason_json` is never populated — sessions always report no termination reason

**File:** `crates/jadepaw-agent/src/loop.rs:345`, `crates/jadepaw-db/src/models.rs:77`
**Issue:** (Previously IN-01 in two prior reviews; now upgraded to Warning due to confirmed data-loss impact on observability and debugging.) The `SessionSnapshot.termination_reason_json` field is defined in the data model, stored in the SQL schema, deserialized in `load()` and `list_by_tenant()` (into `SessionSummary.termination_reason`), but ALL write paths unconditionally set it to `None`:

- `loop.rs:345`: `termination_reason_json: None` — the per-turn checkpoint never populates this.
- There is no "final save" after loop termination that would write the termination reason.

Concrete impact: `SessionSummary.termination_reason` is always `None` for every session in `list_by_tenant()`. This makes it impossible to distinguish sessions that ended normally from those terminated by a guard, and impossible to surface termination reasons in a session management UI. With a populated field, crash recovery (`mark_running_as_paused`) could also distinguish between expected and unexpected pauses.

**Fix:** After `react_loop` returns (both success and error paths in `lib.rs` and `resume_session`), construct the appropriate `AgentTerminationReason`, serialize it via `serde_json::to_string`, and persist it via `repo.save()` or a dedicated final-status update.

### WR-03: `GuardConfig.recent_turns` — redundant public field and public getter method cause ambiguity

**File:** `crates/jadepaw-agent/src/guard.rs:32,48-50`
**Issue:** (Previously IN-02 in prior reviews; now upgraded to Warning because the API surface ambiguity has concrete maintenance risk.) The field `pub recent_turns: u32` and method `pub fn recent_turns(&self) -> u32` are both public with the same name. In Rust, method resolution rules cause the method to shadow the field in method-call position — but field access via dot notation still resolves to the field. This dual-access pattern is error-prone: a developer writing `config.recent_turns` gets the field (which is mutable through `&mut self`), while `config.recent_turns()` calls the method. The two can diverge if any future logic is added to the getter, silently breaking callers that access the field directly.

Having both also violates the principle of single-canonical-access. Either the field should be made private with the method as the sole accessor, or the redundant getter (which is a trivial pass-through) should be removed.

**Fix:** Option A: Remove the method, keep the field public (simplest):
```rust
// Remove lines 46-50 (the recent_turns() method)
```

Option B: Make the field private, use `#[serde(default)]` or `#[serde(rename)]` to maintain serde compatibility:
```rust
#[derive(Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    pub max_iterations: u32,
    pub wall_clock_timeout: Duration,
    recent_turns: u32,  // made private
}

impl GuardConfig {
    pub fn recent_turns(&self) -> u32 {
        self.recent_turns
    }
}
```

## Info

### IN-01: `sqlx::query()` runtime API used instead of `sqlx::query!()` compile-time checked queries

**File:** `crates/jadepaw-db/src/sqlite_repo.rs` (throughout)
**Issue:** All SQL queries use `sqlx::query()` — the runtime-only API that provides no compile-time type checking. The project's own CLAUDE.md recommends SQLx for "compile-time checked SQL" and "compile-time query checking." With `sqlx::query!()`, column name mismatches, type mismatches, and schema drift would be caught at compile time. The current approach defers all SQL errors to runtime.

This is intentional for this codebase phase (sqlx::query!() requires the database to be reachable during compilation, which may not be practical for CI with in-memory SQLite), but documenting the trade-off is important for future maintainers.

**Fix:** Consider migrating to `sqlx::query!()` with a compile-time accessible database (e.g., a test SQLite database checked into the repo, or using the `offline` feature with a prepared query data file).

### IN-02: `AgentRequest.resume_from` field is defined but never consumed by any logic path

**File:** `crates/jadepaw-core/src/agent_types.rs:37`
**Issue:** The `resume_from: Option<SessionId>` field on `AgentRequest` is defined and serialized/deserialized, but no code path in `jadepaw-agent` or `jadepaw-db` reads this field. The decision between `run_agent` (fresh session) and `resume_session` (resumed session) is made by the caller (presumably `jadepaw-gateway`), not by inspecting this field. This creates a subtle API contract: the field is part of the request type but is advisory — the actual routing decision happens at a higher level.

This is not a bug (the field could be consumed by jadepaw-gateway in a future phase), but it is dead code within the reviewed crate boundary. If `resume_from` is truly a gateway concern, it should live in the gateway's request types rather than in `jadepaw-core`.

**Fix:** Either wire `resume_from` into the agent dispatch logic (with proper validation), or move the field to the gateway layer's request type. If kept in core, add a doc comment clarifying that it is consumed by the gateway, not by `jadepaw-agent`.

---

_Reviewed: 2026-06-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_