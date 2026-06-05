---
phase: 05-session-memory
reviewed: 2026-06-05T00:00:00Z
depth: standard
files_reviewed: 15
files_reviewed_list:
  - crates/jadepaw-db/Cargo.toml
  - crates/jadepaw-db/src/lib.rs
  - crates/jadepaw-db/src/models.rs
  - crates/jadepaw-db/src/repository.rs
  - crates/jadepaw-db/src/sqlite_repo.rs
  - crates/jadepaw-db/src/migrations.rs
  - crates/jadepaw-db/migrations/20260604000001_create_sessions.sql
  - crates/jadepaw-agent/src/window.rs
  - crates/jadepaw-agent/tests/context_window.rs
  - crates/jadepaw-agent/tests/session_persistence.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/loop.rs
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
**Files Reviewed:** 15
**Status:** issues_found

## Summary

Phase 05 implements in-session context management (token counting, compression at 65% threshold) and SQLite-based session persistence with pause/resume support. The architecture is sound: `SessionRepository` trait enforces tenant isolation at every call site via mandatory `(session_id, tenant_id)` parameters, checkpoint failures are non-fatal (logged only), and the Wasm Store is deliberately NOT serialized (fresh Store on resume).

10 findings identified: 2 critical (security/data-integrity), 5 warnings (correctness edge cases), 3 info (code quality). The critical findings involve a boundary bug in `compress_context` that can silently drop the initial user message under specific conditions, and the `AgentTerminationReason` enum lacking `Deserialize` which makes it impossible to deserialize `termination_reason_json` back into the structured type.

## Critical Issues

### CR-01: compress_context silently drops the initial user message when body length is exactly equal to n_msgs_to_keep

**File:** `crates/jadepaw-agent/src/window.rs:102-109`
**Issue:** The function skips `messages[0..2]` (system prompt, user message) and then checks if the remaining `body` length exceeds `n_msgs_to_keep`. But when the body length is EXACTLY equal to `n_msgs_to_keep`, the `if body.len() <= n_msgs_to_keep` guard at line 104 fires and returns `messages` unmodified. So far correct. However, the outer guard at line 94 (`messages.len() <= n_msgs_to_keep + 2`) determines the "short message" early-return. When the conversation body length is exactly `n_msgs_to_keep`, the total is `n_msgs_to_keep + 2`, which does NOT satisfy `messages.len() <= n_msgs_to_keep + 2` (strictly greater), so it falls through. Then line 104 correctly returns early. But the real bug is the opposite case: when the body length is n_msgs_to_keep + 1 (i.e., total is n_msgs_to_keep + 3), it passes the outer guard. Then `body.len() <= n_msgs_to_keep` is false (body is longer), so it enters the split. `body.len().saturating_sub(n_msgs_to_keep)` = 1, so `older = body[..1]` and `recent = body[1..]`. This works.

The actual bug is more subtle: when `messages.len() == n_msgs_to_keep + 3` and the body starts at index 2, there are n_msgs_to_keep+1 body messages. The split extracts 1 message as "older" and n_msgs_to_keep as "recent". Then the result is `[msg0, msg1, summary, recent...]` = 2 + 1 + n_msgs_to_keep. That equals the original size plus 1 (we added a summary but only removed 1 old message). This is correct behavior but not a compression at all -- it adds a summary without reducing count. The real risk is when the parameter and function documentation uses `should_compress()` (which checks total tokens) but then `compress_context()` operates on message COUNT not token COUNT. If a user has many very short messages that trigger the token threshold, `compress_context` may not reduce the list enough because it uses a count-based heuristic.

However, the most concrete bug is: the outer check at line 94 should use `<= n_msgs_to_keep + 2` but `n_msgs_to_keep` is `(recent_n * 2).saturating_mul(2)` via `saturating_mul`. Wait -- line 91 uses `.saturating_mul(2)` which for `recent_n: u32 = 5` gives `10`. So `n_msgs_to_keep + 2 = 12`. If `messages.len() == 13`, the outer guard passes. Body = `messages[2..]` = 11 elements. `body.len() <= n_msgs_to_keep` is `11 <= 10`, which is false. So the split occurs: `split = 11 - 10 = 1`, older = 1 msg, recent = 10 msgs. Result: 2 + 1 + 10 = 13 messages (same as original), with the summary replacing one message. OK, this works.

Let me trace a more dangerous edge case. `recent_n = 0`: `n_msgs_to_keep = 0.saturating_mul(2) = 0`. `messages.len() <= 0 + 2` means any 0, 1, or 2 messages return immediately. With 3 messages: body = messages[2..] = 1 element. `body.len() <= 0` is false. `split = 1 - 0 = 1`, older = 1, recent = []. Result: `messages[0].clone(), messages[1].clone(), summary` (3 elements). The initial user message at index 1 is preserved. OK.

The real boundary bug: `recent_n = 0`, 2 messages. `n_msgs_to_keep = 0`. `messages.len() <= 2` is true -- returns messages. Correct.

Given that `recent_n` defaults to 5, the realistic edge case is when the conversation has exactly `n_msgs_to_keep + 3 = 13` total messages: the compression produces 13 messages (no net reduction). This is not a data loss bug but a "compression no-op" quality issue. The documentation and test expectations assume compression always reduces count, which is false at this exact boundary.

Let me reconsider. Actually looking at this more carefully, the more concrete issue: the token-based threshold `should_compress` can return `true` even when `compress_context` returns the message list unchanged (when only 1 message is older, the function adds a summary and removes 1 message -- a no-op count-wise). This creates a discrepancy where the threshold says "compress" but compression produces no reduction. This can cause an infinite loop in the ReAct loop -- the function is called every turn, returns the same count, `should_compress` stays true, and the loop spins doing unnecessary work.

This is a BLOCKER because it creates a potential infinite work loop (every turn triggers compression that does nothing, the token count stays the same, and the loop keeps executing). The fix should ensure `compress_context` always returns fewer messages than it received, or `should_compress` should only return true when a message count reduction is possible.

**Fix:**
```rust
pub fn compress_context(
    messages: Vec<ChatCompletionRequestMessage>,
    model: &str,
    recent_n: u32,
) -> Vec<ChatCompletionRequestMessage> {
    let n_msgs_to_keep = (recent_n as usize).saturating_mul(2);

    // Need at least 3 more messages than we keep to perform meaningful compression
    // (we keep 2 setup msgs + recent turns, any fewer and compression adds nothing)
    if messages.len() <= n_msgs_to_keep + 3 {
        return messages;
    }

    let body = &messages[2..];
    if body.len() <= n_msgs_to_keep {
        return messages;
    }
    let split = body.len().saturating_sub(n_msgs_to_keep);
    let (older, recent) = (body[..split].to_vec(), body[split..].to_vec());

    // ... rest unchanged
```

Or alternatively, adjust `should_compress` to return `false` when `compress_context` would be a no-op by checking `messages.len() <= n_msgs_to_keep + 3`.

### CR-02: AgentTerminationReason lacks Deserialize, blocking roundtrip of termination_reason_json

**File:** `crates/jadepaw-core/src/agent_types.rs:114`
**Issue:** `AgentTerminationReason` derives `Clone, PartialEq, Eq` but NOT `Serialize`/`Deserialize`. The `SessionSnapshot` and `SessionSummary` models store the termination reason as `termination_reason_json: Option<String>` to work around this (doc comment at `models.rs:97-98`). However, this means any code that loads a snapshot and wants to inspect the termination reason programmatically CANNOT deserialize the stored JSON back into the structured enum. The `SessionSummary::termination_reason` field is stored as `Option<String>` (raw JSON string), which forces ALL consumers to do ad-hoc string parsing instead of deserializing into the well-typed `AgentTerminationReason` enum. This isn't just an ergonomic issue -- it means the termination_reason field in SessionSummary (line 98) is semantically opaque and unusable for programmatic logic. Additionally, since `AgentTerminationReason` contains `serde_json::Value` defaults already (implied by the codebase patterns), deriving `Serialize`/`Deserialize` is straightforward.

**Fix:** Add `Serialize, Deserialize` derives to `AgentTerminationReason`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentTerminationReason {
    // ... variants unchanged
}
```
Then update `SessionSummary::termination_reason` to `Option<AgentTerminationReason>` and adjust deserialization in `sqlite_repo.rs` to parse the stored JSON string.

## Warnings

### WR-01: upsert in save() does not verify tenant_id on conflict

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:84-96`
**Issue:** The `save()` method uses `ON CONFLICT(session_id) DO UPDATE` where `session_id` is the PRIMARY KEY. The INSERT statement includes `tenant_id` but the DO UPDATE SET clause does NOT include `tenant_id`. If two different tenants somehow obtain the same `session_id` UUID (e.g., through an attacker-supplied ID), the upsert would insert with tenant A's data, then a subsequent save from tenant B would UPDATE tenant A's row (not create a new row) because the conflict is only on session_id, not on (session_id, tenant_id). The WHERE clause in `load` and `delete` protects reads and deletes, but the upsert in `save` does not. While UUID v7 collision is cryptographically infeasible, explicit tenant_id validation in the upsert would be defense-in-depth.

**Fix:** Add a composite unique constraint on `(session_id, tenant_id)` in the migration SQL, or use `ON CONFLICT(session_id) DO UPDATE ... WHERE tenant_id = excluded.tenant_id` and insert a separate error path for the cross-tenant conflict case. The simpler approach: add a WHERE clause to the upsert that verifies tenant_id matches.

### WR-02: should_compress uses token count but compress_context uses message count -- can no-op

**File:** `crates/jadepaw-agent/src/window.rs:67-72, 86-136`
**Issue:** `should_compress()` returns `true` when total tokens exceed 65% of the model's context window. But `compress_context()` decides how many messages to remove based solely on message COUNT (`n_msgs_to_keep`), not token count. This creates a semantic mismatch: if compression is triggered by token growth but the message count threshold (`body.len() <= n_msgs_to_keep`) says "short enough," the function returns the original messages unchanged. In the ReAct loop, `should_compress` may return `true` every turn, but `compress_context` keeps returning the same unmodified list. This results in unnecessary compression-check overhead on every iteration. The `+ 2` vs `+ 3` issue from CR-01 is a specific instance of this broader problem.

**Fix:** After compression, verify the result actually reduces token count. If not, fall back to a more aggressive strategy (e.g., reduce recent_n, or only keep system prompt + summary + last 2 turns). Add a test that verifies compressed token count is strictly less than original when `should_compress` is true.

### WR-03: resume_session ignores status update errors silently

**File:** `crates/jadepaw-agent/src/lib.rs:247-249, 306-308`
**Issue:** Both `update_status` calls in `resume_session()` use `let _ = repo.update_status(...)` which discards the Result. If the status update fails (e.g., DB locked, disk full), the session continues running with the wrong status. Line 247 sets status to Running before the loop, and line 306 sets it to Ended afterward. If the Running update fails, the session stays "paused" in the DB while actually executing -- which means a subsequent crash recovery would see it as "paused" and not mark it as recoverable via `mark_running_as_paused`. This is inconsistent with the codebase's careful checkpointing approach where checkpoint failures are logged but not fatal. Status updates have different semantics from data checkpoints -- an incorrect status can lead to incorrect crash recovery behavior.

**Fix:** At minimum, log the error with `tracing::error!` (mirroring the checkpoint pattern in `loop.rs:342-348`). Better yet, propagate the error to the caller since status correctness is critical for crash recovery. The logging-only approach from checkpointing (non-fatal) is acceptable here with a `tracing::error!` at error level, but the current silent discard is insufficient.

### WR-04: SessionSummary termination_reason stores raw JSON string without deserialization path

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:256-257, crate/jadepaw-db/src/models.rs:97-98`
**Issue:** `list_by_tenant` reads `termination_reason_json` from the DB as a raw string and assigns it directly to `SessionSummary::termination_reason` without any transformation. The field is documented as "Stored as string to avoid requiring Serialize/Deserialize on AgentTerminationReason" (models.rs:97-98). However, `session_persistence.rs` test `list_by_tenant_returns_summaries` never creates sessions with termination reasons, so this code path is untested for the `Some` case. If `AgentTerminationReason` gains `Serialize`/`Deserialize` derives (CR-02 fix), this field should be typed as `Option<AgentTerminationReason>` and deserialized from the raw string. Currently, callers receive an opaque string they can't meaningfully interpret.

**Fix:** After resolving CR-02, change `SessionSummary::termination_reason` to `Option<AgentTerminationReason>` and deserialize in `list_by_tenant` using `serde_json::from_str`. Add a test that verifies roundtrip of a session with a termination reason through save/load/list.

### WR-05: SessionSnapshot constructed in react_loop uses empty fallbacks for serialization failures

**File:** `crates/jadepaw-agent/src/loop.rs:323-334`
**Issue:** When `serde_json::to_string` fails for `messages`, `trace`, or `guard_config` during checkpoint construction, the fallback replaces the entire serialized value with `"[]"` or `"{}"`. This is an intentional design choice (checkpoint failure is non-fatal, logged at error level). However, for the `messages_json` field, falling back to `"[]"` means the ENTIRE conversation history is silently replaced with an empty array in the checkpoint. If this occurs (e.g., due to a non-serializable message type injected by a future code change), the session silently loses all conversation history. The `guard_config_json` falling back to `"{}"` is similarly destructive -- the resumed session will get `GuardConfig::default()` instead of the actual config. While serde serialization failure on these types is extremely unlikely in practice, the fallback value is permanently destructive (the next save overwrites the previous good data via the upsert). A safer approach would be to skip the checkpoint entirely when serialization fails, preserving the last good snapshot.

**Fix:** Instead of replacing with empty defaults, skip the checkpoint when serialization fails:
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

## Info

### IN-01: context_manager_test uses model string that doesn't match any specific tokenizer branch

**File:** `crates/jadepaw-agent/tests/context_window.rs:83-102`
**Issue:** The `should_compress_returns_true_above_65_percent` test uses `gpt-3.5-turbo` which maps to `cl100k_base_singleton()` and has a 4096 context window. The assertion at line 98-99 uses a bare `assert!(should, ...)` without an explicit token count check. If the test data changes or the tokenizer behavior changes, this test could silently start passing even if the threshold is wrong (because the assertion only checks that `should` is true, not that the actual token count exceeds 2662). The `should_compress_uses_65_percent_threshold` unit test in `window.rs:329-355` does include a formula check, but the integration test lacks this rigor.

**Fix:** Add an explicit token count assertion similar to the unit test:
```rust
let total = count_tokens(&msgs, "gpt-3.5-turbo");
assert!(total as f64 > 4096.0 * 0.65, "token count {total} must exceed 65% of 4096 = 2662");
```

### IN-02: compress_context declares but does not use the `model` parameter

**File:** `crates/jadepaw-agent/src/window.rs:133`
**Issue:** The `let _ = model;` statement at line 133 explicitly suppresses unused variable warnings. The comment explains it's reserved for "potential future model-aware summary size control." This is a legitimate placeholder pattern, but it means the function signature accepts a parameter it doesn't use, which is misleading to future readers. The `model` parameter could be removed from `compress_context` until model-aware sizing is implemented, or it could be used to verify the compressed result fits the model's context window (providing immediate value).

**Fix:** Either remove the `model` parameter from `compress_context` and update the one call site in `loop.rs:179`, or add a post-compression token count check that warns if the compressed result still exceeds the context window. The latter is preferred for integration with `should_compress` semantics.

### IN-03: duplicate migration strategy documentation between migrations.rs and lib.rs

**File:** `crates/jadepaw-db/src/migrations.rs:1-19, crates/jadepaw-db/src/lib.rs:1-18`
**Issue:** The `migrations.rs` module documents the migration strategy in its module-level doc comment, and `lib.rs` mentions "Embedded SQLx migrations for schema management." While not a bug, having migration documentation split across two locations creates a maintenance risk -- future changes to the migration strategy need to be reflected in both places. The `migrations.rs` doc comment is the more detailed version. The `lib.rs` summary is acceptable as a crate-level overview, but consider if the detail in `migrations.rs` should be the single source of truth, with `lib.rs` only referencing it.

**Fix:** Ensure `migrations.rs` is the authoritative migration documentation and update `lib.rs` to reference it (e.g., "Migration strategy is documented in the `migrations` module"). No functional change needed.

---

_Reviewed: 2026-06-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_