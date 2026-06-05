---
phase: 05-session-memory
reviewed: 2026-06-05T18:00:00Z
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
  critical: 2
  warning: 3
  info: 3
  total: 8
status: issues_found
---

# Phase 05: Code Review Report (Adversarial Re-review)

**Reviewed:** 2026-06-05
**Depth:** standard
**Files Reviewed:** 16
**Status:** issues_found

## Summary

This is the third adversarial re-review of the Phase 05 (session-memory) implementation. Two previous fix iterations addressed 15 findings total (7 Critical+Warning in iteration 1, 2 Warning+5 Info in iteration 2, and a further 2 Warning in iteration 3). The code has steadily improved and is approaching production quality.

This review identifies 2 new Critical findings (one correctness bug in test infrastructure, one data integrity issue in persistence) and 3 new Warnings (inconsistent integer truncation patterns, overflow risk in elapsed time computation, missing guard config validation). Three existing Info items are reaffirmed.

## Critical Issues

### CR-01: In-memory SQLite URI is a file-path, not an in-memory database

**File:** `crates/jadepaw-agent/tests/session_persistence.rs:32`
**Issue:** The test helper `make_repo()` creates a SQLite database with `SqliteSessionRepo::new("sqlite://:memory:")`. In SQLx's URI parser, `"sqlite://:memory:"` is interpreted as a file path: a file literally named `:memory:` in the current working directory. This means:
1. Tests are not truly isolated — a database file named `:memory:` is created on disk and persists across test runs.
2. Subsequent test runs may fail or produce incorrect results if the file from a prior run still exists.
3. The database is not ephemeral, violating the explicit test design intent.

The correct SQLx URI for an in-memory SQLite database is `"sqlite::memory:"` (note the double colon) or `":memory:"` (bare). The `SqliteConnectOptions::from_str("sqlite://:memory:")` call in `SqliteSessionRepo::new()` treats the path component as a filesystem path, not as the SQLite `:memory:` special name.

**Fix:**
```rust
async fn make_repo() -> SqliteSessionRepo {
    SqliteSessionRepo::new("sqlite::memory:")
        .await
        .expect("failed to create in-memory SQLite repo")
}
```

### CR-02: Elapsed time uses `as u64` narrowing cast on `u128`, silently truncating on overflow

**File:** `crates/jadepaw-agent/src/loop.rs:318`
**Issue:** The `react_loop` function computes elapsed time for persisting checkpoints:
```rust
let elapsed = elapsed_accumulator_ms
    + start.elapsed().as_millis() as u64;
```
`Instant::elapsed().as_millis()` returns `u128`. The `as u64` cast silently truncates the upper 64 bits on overflow. While the practical likelihood is low (a u64 of milliseconds is ~584 million years), the same codebase in `guard.rs:149-150` correctly uses `u64::try_from(...).unwrap_or(u64::MAX)`, establishing an inconsistency:
```rust
// guard.rs:149 (correct approach)
let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
```
This inconsistency between two time-computation sites in the same crate creates a maintenance hazard — if a developer copies the buggy pattern from `loop.rs`, they may introduce a silently incorrect computation in a more sensitive path.

**Fix:** Apply the same `try_from` pattern used in `guard.rs`:
```rust
let elapsed = elapsed_accumulator_ms
    + u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
```

## Warnings

### WR-01: `iteration_count: turn + 1` unsigned overflow risk in guard-covered code paths

**File:** `crates/jadepaw-agent/src/loop.rs:342`
**Issue:** The checkpoint snapshot uses `iteration_count: turn + 1`. Both `turn` and `iteration_count` are `u32`. The `turn` variable is bounded by the for loop range `start_turn..guard_config.max_iterations` where `max_iterations` is `u32`. If `max_iterations` equals `u32::MAX`, `turn + 1` would overflow to 0 on the last iteration. While `GuardConfig::default()` sets `max_iterations` to 20 and the serialize path ensures only valid values reach the DB, `GuardConfig` is deserialized from user input at runtime (`serde_json::from_str(&snapshot.guard_config_json)`) and no validation enforces reasonable bounds on `max_iterations`. A crafted guard config with `max_iterations: u32::MAX` would produce incorrect iteration counts in checkpoints.

**Fix:** Use `saturating_add` or validate `max_iterations` at deserialization:
```rust
// In GuardConfig or its deserialization point:
pub fn validate(&self) -> Result<(), &'static str> {
    if self.max_iterations == 0 {
        return Err("max_iterations must be positive");
    }
    if self.max_iterations > 1_000_000 {
        return Err("max_iterations exceeds reasonable limit");
    }
    Ok(())
}
```
Or at minimum, at the checkpoint site:
```rust
iteration_count: turn.saturating_add(1),
```

### WR-02: SQLite BLOB-to-integer cast silently wraps negative values to large positive u64

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:178,200,250,288`
**Issue:** The `load()` function reads `elapsed_ms` as `i64` from SQLite (line 178: `let elapsed_ms: i64 = row.get("elapsed_ms")`) then unconditionally casts to `u64` at line 200: `elapsed_ms: elapsed_ms as u64`. The same pattern repeats in `list_by_tenant()` (lines 250, 288). If a negative value were stored in the database (via manual manipulation, a bug in a future migration, or data corruption), this cast would silently produce a very large `u64` (e.g., `-1_i64 as u64 = 18446744073709551615`), resulting in the code believing sessions have been running for ~584 million years.

While the Rust-side code never stores negative values (the model uses `u64` and the DB binding casts `u64 as i64`), defense in depth dictates that the read path should validate its invariants rather than silently producing garbage from corrupted data.

**Fix:** Use `try_from` with an error or clamp:
```rust
let elapsed_ms: i64 = row.get("elapsed_ms");
let elapsed_ms = u64::try_from(elapsed_ms).context("elapsed_ms in database is negative or exceeds u64")?;
```
Or for `list_by_tenant()` where failing the entire query for one bad row may be undesirable:
```rust
let elapsed_ms: i64 = row.get("elapsed_ms");
let elapsed_ms = u64::try_from(elapsed_ms).unwrap_or(0);
```

### WR-03: `created_at` remains `session_created_at` across all checkpoint saves, losing save-time semantics in `updated_at`

**File:** `crates/jadepaw-agent/src/loop.rs:343`
**Issue:** The checkpoint snapshot constructed at each turn uses `created_at: session_created_at` — the timestamp from when the session was initially created. The `updated_at` field is correctly set to `chrono::Utc::now()`. This is semantically correct: `created_at` should be invariant. However, the `save()` UPSERT in `sqlite_repo.rs:90-102` uses `ON CONFLICT(session_id) DO UPDATE SET ...` which does NOT update the `created_at` column. This means if the session is loaded from a previous checkpoint and re-saved, the original `created_at` is preserved — which is correct. No bug here, but worth documenting why this is intentional (the field name `created_at` makes it obvious, but the UPSERT does not set it, relying on the INSERT path only).

This is acknowledged as not a bug but a documentation gap. No fix required.

## Info

### IN-01: `SessionSnapshot.termination_reason_json` is never populated on write paths (recurring)

**File:** `crates/jadepaw-agent/src/loop.rs:345`, `crates/jadepaw-db/src/models.rs:77`

**Issue:** (Previously IN-03 in prior review, now IN-01). The `SessionSnapshot::termination_reason_json` field is defined in the data model, present in the SQL schema, and properly read back in `load()` and `list_by_tenant()`. However, all write paths set it to `None`. The field is never populated with an actual `AgentTerminationReason` when a session ends, meaning `SessionSummary::termination_reason` will always be `None` for all sessions.

**Fix:** When the agent loop terminates (e.g., `react_loop` returns `Ok(trace)`) or the guard fires, construct the appropriate `AgentTerminationReason`, serialize it, and persist it in a final checkpoint.

### IN-02: `GuardConfig.recent_turns` — redundant public field and public getter method

**File:** `crates/jadepaw-agent/src/guard.rs:32,48-50`

**Issue:** (Previously IN-02 in prior review). The field `pub recent_turns: u32` (line 32) and the method `pub fn recent_turns(&self) -> u32` (line 48) are both public with the same name. The method shadows the field in method-call position due to Rust's resolution rules. Having both is redundant and confusing.

**Fix:** Remove the method (keep field access since `serde` can use private fields for deserialization) or make the field private and keep the method for proper encapsulation.

### IN-03: `elapsed_ms` grows monotonically on each turn, but the accumulator is never bounded

**File:** `crates/jadepaw-agent/src/loop.rs:317-318`

**Issue:** The checkpoint `elapsed_ms` is computed as `elapsed_accumulator_ms + start.elapsed().as_millis()`. With `elapsed_ms` declared as `u64` in the database model, extremely long-lived sessions (theoretical, given wall_clock_timeout defaults to 300s) could not overflow this. However, the `iteration_count` is stored as `i32` in SQLite (INTEGER column), and `elapsed_ms` as `i64`. Both have practical limits far above normal values, but the schema declares INTEGER without explicit size constraints, making the limits implicit.

**Fix:** Document the practical upper bounds in the model comments, or add defensive checks in the checkpoint construction that cap values at the DB column's maximum representable value.

---

_Reviewed: 2026-06-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_