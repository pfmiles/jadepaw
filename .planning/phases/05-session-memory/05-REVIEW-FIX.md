---
phase: 05-session-memory
fixed_at: 2026-06-05T00:00:00Z
review_path: .planning/phases/05-session-memory/05-REVIEW.md
iteration: 1
findings_in_scope: 7
fixed: 7
skipped: 0
status: all_fixed
---

# Phase 05: Code Review Fix Report

**Fixed at:** 2026-06-05
**Source review:** .planning/phases/05-session-memory/05-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope (Critical + Warning): 7
- Fixed: 7
- Skipped: 0

## Fixed Issues

### CR-01: compress_context can no-op at the `n_msgs_to_keep + 3` boundary, causing infinite re-compression in the ReAct loop

**Files modified:** `crates/jadepaw-agent/src/window.rs`
**Commit:** `bab77d3`
**Applied fix:**
- Changed the early-return guard from `n_msgs_to_keep + 2` to `n_msgs_to_keep + 3` to guarantee at least one message is always removed.
- Added post-compression message-count verification: if `result.len() >= messages.len()`, apply aggressive fallback (summary + recent N turns only, dropping original system/user messages).
- Added post-compression token-count verification: if `compressed_tokens >= original_tokens`, apply the same aggressive fallback.
- Replaced the dead `let _ = model;` with actual model-based token counting using `count_tokens()`.

### CR-02: upsert `ON CONFLICT(session_id)` allows cross-tenant data overwrite when session_id collides

**Files modified:** `crates/jadepaw-db/src/sqlite_repo.rs`
**Commit:** `94411c3`
**Applied fix:**
- Added `WHERE tenant_id = excluded.tenant_id` to the `ON CONFLICT DO UPDATE` clause so an existing row with a different tenant is not silently overwritten.
- Added `rows_affected() == 0` check after the upsert to detect cross-tenant collisions and surface them as an `anyhow::bail!` error.

### WR-01: resume_session silently discards status update errors, risking crash recovery inconsistency

**Files modified:** `crates/jadepaw-agent/src/lib.rs`
**Commit:** `5344622`
**Applied fix:**
- Replaced `let _ = repo.update_status(...)` with `if let Err(e) = ... { tracing::error!(...) }` for both Running (line 247) and Ended (line 306) status updates.
- Error messages include session_id, tenant_id, and context about crash recovery impact.

### WR-02: serialize-to-"{}" fallback for guard_config_json will silently revert resumed sessions to default GuardConfig

**Files modified:** `crates/jadepaw-agent/src/loop.rs`
**Commit:** `6ebb481`
**Applied fix:**
- Replaced three `unwrap_or_else` fallback blocks (messages_json to `"[]"`, trace_json to `"[]"`, guard_config_json to `"{}"`) with `let Ok(...) = ... else { tracing::error!(...); continue; }` patterns.
- On serialization failure, the turn's checkpoint is skipped entirely, preserving the last good snapshot in the database.

### WR-03: should_compress uses token count but compress_context uses message count -- semantic mismatch

**Files modified:** `crates/jadepaw-agent/src/window.rs`
**Commit:** `bab77d3` (same commit as CR-01)
**Applied fix:**
- Added post-compression token-count verification in `compress_context` using the `model` parameter (previously unused).
- If compressed tokens are not less than original tokens, applies aggressive fallback to guarantee progress.
- Resolved by the same changes as CR-01.

### WR-04: extract_turn_from_error caller uses unwrap_or(0), defeating the Option design

**Files modified:** `crates/jadepaw-agent/src/guard.rs`
**Commit:** `1400450`
**Applied fix:**
- Replaced `extract_turn_from_error(&err_msg).unwrap_or(0)` with a `match` expression.
- `Some(turn)` uses the extracted turn normally.
- `None` uses `u32::MAX` as a sentinel to indicate "turn could not be parsed", distinguishing it from a genuine turn-0 error.
- Also reclassified the fallback error path from `WasmTrap` to `InfrastructureError` (unknown errors are host-side, not guest traps).

### WR-05: SessionSummary.termination_reason stores raw JSON string without a deserialization path

**Files modified:** `crates/jadepaw-core/src/agent_types.rs`, `crates/jadepaw-db/src/models.rs`, `crates/jadepaw-db/src/sqlite_repo.rs`
**Commit:** `b35e58b`
**Applied fix:**
- Added `Serialize, Deserialize` derives to `AgentTerminationReason` in `jadepaw-core`.
- Changed `SessionSummary::termination_reason` from `Option<String>` to `Option<AgentTerminationReason>`.
- Updated `list_by_tenant` to deserialize the stored JSON string into the typed enum via `serde_json::from_str`.

## Skipped Issues

None -- all 7 in-scope findings were fixed.

---

_Fixed: 2026-06-05_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_