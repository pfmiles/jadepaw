---
phase: 05-session-memory
reviewed: 2026-06-05T00:00:00Z
depth: standard
files_reviewed: 17
files_reviewed_list:
  - Cargo.toml
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
  - crates/jadepaw-db/src/lib.rs
  - crates/jadepaw-db/src/migrations.rs
  - crates/jadepaw-db/src/models.rs
  - crates/jadepaw-db/src/repository.rs
  - crates/jadepaw-db/src/sqlite_repo.rs
  - crates/jadepaw-db/migrations/20260604000001_create_sessions.sql
findings:
  critical: 2
  warning: 5
  info: 3
  total: 10
status: issues_found
---

# Phase 05: Code Review Report

**Reviewed:** 2026-06-05
**Depth:** standard
**Files Reviewed:** 17
**Status:** issues_found

## Summary

Phase 05 implements in-session context management (token counting, compression at 65% threshold) and SQLite-based session persistence with pause/resume support. The architecture is sound: `SessionRepository` trait enforces tenant isolation at every call site via mandatory `(session_id, tenant_id)` parameters, checkpoint failures are non-fatal (logged only), and the Wasm Store is deliberately NOT serialized (fresh Store on resume).

10 findings identified: 2 critical (data-integrity / crash-recovery correctness), 5 warnings (correctness edge cases and defensive gaps), 3 info (code quality). The critical findings involve a context compression no-op that can trigger an infinite work loop when `compress_context` is called on a message list at the exact boundary where it cannot reduce message count, and an upsert in the SQLite save path that does not verify `tenant_id` on conflict — allowing cross-tenant data corruption in the presence of colliding session IDs.

## Critical Issues

### CR-01: compress_context can no-op at the `n_msgs_to_keep + 3` boundary, causing infinite re-compression in the ReAct loop

**File:** `crates/jadepaw-agent/src/window.rs:86-136`
**Issue:** `should_compress()` uses token count to decide when to compress (line 67-72), but `compress_context()` uses message COUNT to determine how many messages to remove (line 91: `n_msgs_to_keep = recent_n * 2`). This creates a mismatch: when a conversation has exactly `n_msgs_to_keep + 3` total messages (e.g., `recent_n = 5` -> `n_msgs_to_keep = 10` -> 13 total messages), the outer guard at line 94 (`messages.len() <= n_msgs_to_keep + 2`) fails (13 > 12), so compression proceeds. The `body.len()` is 11, which is > 10, so the split executes: `split = 1`, `older = 1 msg`, `recent = 10 msgs`. The result is `[msg0, msg1, summary, recent...]` = 13 messages, which is the SAME count as the original. No reduction occurs.

If `should_compress()` returned `true` (token count exceeded threshold) but `compress_context()` returns the same-size message list, the token count will still exceed the threshold on the next iteration. The ReAct loop at `loop.rs:176-186` calls `should_compress` every turn and invokes `compress_context` when it returns `true`. This creates an infinite busy-work cycle: every turn compresses, achieves nothing, token count stays the same, and the next turn compresses again.

**Fix:**
Ensure `compress_context` never returns the original message count. Change the guard at line 94 from `+ 2` to `+ 3`, which guarantees at least one message is removed (the replaced old messages must outnumber the summary addition):

```rust
    // Need at least 3 more messages than we can keep for meaningful compression.
    // Otherwise we'd add a summary message but only "remove" a single old message,
    // leaving the total count unchanged and causing infinite re-compression.
    if messages.len() <= n_msgs_to_keep + 3 {
        return messages;
    }
```

Alternatively, add a post-compression token count check and fall back to a more aggressive strategy (e.g., drop original system/user messages) if the compressed result still exceeds the threshold.

### CR-02: upsert `ON CONFLICT(session_id)` allows cross-tenant data overwrite when session_id collides

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:84-96`
**Issue:** The `save()` method uses `INSERT ... ON CONFLICT(session_id) DO UPDATE` where `session_id` alone is the PRIMARY KEY. The INSERT includes `tenant_id`, but the DO UPDATE SET clause does NOT update `tenant_id` on conflict — yet it DOES overwrite all other fields (`messages_json`, `trace_json`, etc.) belonging to the original tenant. The WHERE clause in `load()` and `delete()` protects reads and deletes via `WHERE session_id = ? AND tenant_id = ?`, but the upsert in `save()` has no such protection.

This means: if two different tenants produce the same `session_id` UUID (through a v7 collision, external injection, or UUID reuse bug), tenant B's save will silently overwrite tenant A's session data because the conflict is only on `session_id`, not `(session_id, tenant_id)`. While UUID v7 collision is cryptographically infeasible in normal operation, the defense-in-depth principle already applied everywhere else in this codebase (mandatory dual-key lookups) is violated right at the persistence layer.

**Fix:**
Add a composite unique constraint on `(session_id, tenant_id)` in the migration SQL, then use it in the upsert:

```sql
-- In migration: change primary key to composite or add a unique constraint
ALTER TABLE sessions DROP PRIMARY KEY;
ALTER TABLE sessions ADD PRIMARY KEY (session_id, tenant_id);
```

If keeping `session_id` as the sole primary key is desired for query performance, use a WHERE clause in the upsert:

```rust
sqlx::query(
    "INSERT INTO sessions (...) VALUES (...)\
     ON CONFLICT(session_id) DO UPDATE SET \
       status = excluded.status, \
       messages_json = excluded.messages_json, \
       ... \
       WHERE tenant_id = excluded.tenant_id"
)
```

Then add a separate (or fallback) error path when the WHERE clause excludes the row (meaning the tenant_id didn't match — a genuine collision scenario that should be surfaced as an error).

## Warnings

### WR-01: resume_session silently discards status update errors, risking crash recovery inconsistency

**File:** `crates/jadepaw-agent/src/lib.rs:247-249, 306-308`
**Issue:** Both `update_status` calls in `resume_session()` use `let _ = repo.update_status(...)` which discards the `Result`. Line 247 sets status to Running before the loop, and line 306 sets it to Ended afterward. If the Running update fails (e.g., DB locked, disk full), the session stays "paused" in the DB while actually executing — meaning a subsequent crash recovery via `mark_running_as_paused()` would NOT detect this session as recoverable because it's still marked "paused". The checkpointing pattern in `loop.rs:342-348` logs checkpoint failures at error level. Status updates have different semantics: an incorrect status leads to incorrect crash recovery behavior, which is more severe than a missed checkpoint.

**Fix:**
At minimum, log with `tracing::error!` (mirroring the checkpoint pattern). Better yet, propagate the error:

```rust
    repo.update_status(session_id, tenant_id, SessionStatus::Running)
        .await
        .map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("failed to update session status to running: {}", e),
                    turn: 0,
                },
            )
        })?;
```

### WR-02: serialize-to-"{}" fallback for guard_config_json will silently revert resumed sessions to default GuardConfig

**File:** `crates/jadepaw-agent/src/loop.rs:331-334`
**Issue:** When `serde_json::to_string(guard_config)` fails during checkpoint construction, the fallback stores `"{}"`. On resume at `lib.rs:243-244`, `serde_json::from_str(&snapshot.guard_config_json)` will fail on `"{}"` (missing fields), and `unwrap_or_default()` will silently produce `GuardConfig::default()`. If the session was running with a non-default config (e.g., `max_iterations: 50`, custom `wall_clock_timeout`), the resumed session silently switches to defaults. This is a silent correctness degradation. While serde serialization failure on GuardConfig is extremely unlikely, the fallback value is permanently destructive: the next checkpoint save will overwrite the previous good data via the upsert.

**Fix:**
Instead of falling back to empty defaults, skip the checkpoint when serialization fails:

```rust
let Ok(messages_json) = serde_json::to_string(&messages) else {
    tracing::error!("failed to serialize messages for checkpoint; skipping");
    continue;
};
let Ok(trace_json) = serde_json::to_string(&trace) else {
    tracing::error!("failed to serialize trace for checkpoint; skipping");
    continue;
};
let Ok(guard_config_json) = serde_json::to_string(guard_config) else {
    tracing::error!("failed to serialize guard config for checkpoint; skipping");
    continue;
};
```

This preserves the last good snapshot instead of overwriting it with bad data.

### WR-03: should_compress uses token count but compress_context uses message count — semantic mismatch

**File:** `crates/jadepaw-agent/src/window.rs:67-72, 86-136`
**Issue:** `should_compress()` returns `true` when total tokens exceed 65% of the model's context window (token-based). But `compress_context()` decides how many messages to remove based solely on message COUNT (`n_msgs_to_keep`). If compression is triggered by token growth (many long messages) but the message count says "short enough," the function returns the original messages unchanged. CR-01 describes the specific infinite-loop case, but the broader issue is that `compress_context` does not verify that its output actually reduces the token count. A session with very long messages could trigger `should_compress` every turn, but `compress_context` only trims message count, not token count per message.

**Fix:**
Add a post-compression token count verification:

```rust
    // Verify compression actually reduced tokens
    let compressed_tokens = count_tokens(&result, model);
    let original_tokens = count_tokens(&messages, model);
    if compressed_tokens >= original_tokens {
        tracing::warn!(
            original = original_tokens,
            compressed = compressed_tokens,
            "compression did not reduce token count; applying aggressive fallback"
        );
        // Fallback: summary only + last 2 turns
        // ...
    }
```

### WR-04: extract_turn_from_error caller uses unwrap_or(0), defeating the Option design

**File:** `crates/jadepaw-agent/src/guard.rs:120`
**Issue:** `extract_turn_from_error` (line 153-161) returns `Option<u32>` with the explicit documented purpose: "callers can distinguish between 'error on turn 0' and 'turn could not be parsed'." However, line 120 uses `unwrap_or(0)`, which collapses the distinction. An unparseable error message is attributed to turn 0, indistinguishable from a real turn-0 error. This is the behavior the `Option` return was explicitly designed to avoid.

**Fix:**
Use `None` to propagate the unknown-turn case. In the `WasmTrap` construction, use a sentinel value or explicitly handle the unparseable case:

```rust
                let err_msg = e.to_string();
                let turn = extract_turn_from_error(&err_msg).unwrap_or(u32::MAX);
                JadepawError::agent_terminated(
                    AgentTerminationReason::WasmTrap {
                        reason: err_msg,
                        turn,
                    },
                )
```

Alternatively, match on `Some(turn)` / `None` and produce a distinct `AgentTerminationReason` variant for unparseable errors.

### WR-05: SessionSummary.termination_reason stores raw JSON string without a deserialization path

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:256-257, crates/jadepaw-db/src/models.rs:97-98`
**Issue:** `list_by_tenant` reads `termination_reason_json` as a raw string and assigns it directly to `SessionSummary::termination_reason`. The field is documented as "Stored as string to avoid requiring Serialize/Deserialize on AgentTerminationReason" (models.rs:97-98). This means ALL consumers of `SessionSummary` receive an opaque string they cannot programmatically interpret — they would need their own ad-hoc JSON parsing, which defeats the purpose of a structured type. The `session_persistence.rs` test `list_by_tenant_returns_summaries` never creates sessions with termination reasons, so the `Some` code path is untested.

**Fix:**
Add `Serialize, Deserialize` derives to `AgentTerminationReason`, change `SessionSummary::termination_reason` to `Option<AgentTerminationReason>`, and deserialize in `list_by_tenant`:

```rust
let termination_reason: Option<AgentTerminationReason> = match termination_reason_json {
    Some(s) => serde_json::from_str(&s).ok(),
    None => None,
};
```

Add a test that creates a session with a termination reason and verifies roundtrip through save/load/list.

## Info

### IN-01: compress_context accepts `model` parameter it does not use

**File:** `crates/jadepaw-agent/src/window.rs:133`
**Issue:** The `let _ = model;` statement explicitly suppresses the unused variable warning. The comment explains it's reserved for "future model-aware summary size control." The parameter is misleading because the function signature suggests model-specific behavior but none is implemented. It also forces the caller (`loop.rs:179`) to thread a `model` string through, adding parameter passing complexity for no runtime benefit.

**Fix:** Either remove the `model` parameter from `compress_context` and update the call site, or implement a post-compression token count check using `model` (providing immediate value: verify compressed result actually fits the model's limits).

### IN-02: migration path documented incorrectly in migrations.rs

**File:** `crates/jadepaw-db/src/migrations.rs:18`
**Issue:** The comment states `sqlx::migrate!("../migrations")` but the actual invocation in `sqlite_repo.rs:62` uses `sqlx::migrate!("./migrations")`. The `"../migrations"` path would resolve one directory above the crate root (`crates/migrations/`) and would fail at compile time. The code is correct — only the comment is wrong.

**Fix:** Update the comment:
```rust
// Migrations are embedded via sqlx::migrate!("./migrations") in sqlite_repo.rs.
```

### IN-03: GuardConfig.recent_turns field and method have the same name

**File:** `crates/jadepaw-agent/src/guard.rs:32,48`
**Issue:** The public field `recent_turns` (line 32) and the public method `recent_turns()` (line 48) have identical names. The method is a trivial getter that returns `self.recent_turns`. Rust allows this (methods shadow fields in method call position), but it's redundant: either the field should be private (if encapsulation is desired) or the method should be removed (since public field access is equivalent).

**Fix:** Remove the method (callers already access `config.recent_turns` as a field) or make the field private and keep the method.

---

_Reviewed: 2026-06-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_