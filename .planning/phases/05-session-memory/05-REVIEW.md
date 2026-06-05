---
phase: 05-session-memory
reviewed: 2026-06-05T14:30:00Z
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
  critical: 0
  warning: 6
  info: 4
  total: 10
status: issues_found
---

# Phase 05: Code Review Report (Re-review after fixes)

**Reviewed:** 2026-06-05
**Depth:** standard
**Files Reviewed:** 17
**Status:** issues_found

## Summary

This is a re-review of the Phase 05 (session-memory) implementation after all 7 Critical + Warning findings from the initial review were fixed (see `05-REVIEW-FIX.md`). The fixes addressed the infinite re-compression loop (CR-01), cross-tenant upsert vulnerability (CR-02), silent status-error swallowing (WR-01), guard-config serialization fallback corruption (WR-02), token-vs-message-count mismatch (WR-03), sentinel-vs-zero ambiguity (WR-04), and raw-string termination-reason (WR-05).

All fixes are verified correct and intact in the current code. This re-review identifies 6 new Warnings and 4 Info items that remain in the codebase or were introduced by the fix iterations. No new Critical/Blocker-level issues were found.

## Warnings

### WR-01: compress_context aggressive fallback drops original system prompt, losing tool definitions and ReAct instructions

**File:** `crates/jadepaw-agent/src/window.rs:147-155, 170-176`
**Issue:** When the post-compression message-count or token-count verification fails (lines 140, 163), the aggressive fallback constructs a new message list as `[summary_msg, recent_n_turns...]` — explicitly dropping `messages[0]` (system prompt with tool definitions and ReAct instructions) and `messages[1]` (original user message). The summary message (`ChatCompletionRequestSystemMessage`) contains extracted statistics from older turns but does NOT replicate the system prompt's critical content (ReAct reasoning format, available tool names and parameter schemas, behavioral constraints).

If the fallback triggers during an active session, the LLM loses awareness of:
- The ReAct think-act-observe format instructions
- Available tool names and their parameter schemas
- Any custom system-level behavioral constraints

This can cause the agent to stop following the ReAct pattern, hallucinate tool names, or produce degraded responses. The `tracing::warn!` call is present, so operators are notified, but the behavioral degradation is silent from the user's perspective.

Note: this only fires in edge cases where normal compression fails to reduce message count or token count — typically when the summary message itself is very large or the recent N turns are enormous. The summary generation is lightweight (~200 chars), so the common case is safe.

**Fix:** Preserve the original system prompt in the aggressive fallback:
```rust
// Instead of dropping messages[0], include it before the summary:
let mut fallback = Vec::with_capacity(2 + recent_len);
fallback.push(messages[0].clone()); // preserve original system prompt
fallback.push(summary_msg);         // inject compression summary
let body = &messages[2..];
let split = body.len().saturating_sub(n_msgs_to_keep);
fallback.extend(body[split..].iter().cloned());
return fallback;
```

### WR-02: build_summary counts ChatCompletionRequestMessage::System variants as "assistant" messages

**File:** `crates/jadepaw-agent/src/window.rs:218-226`
**Issue:** The `assistant_count` computation uses a `matches!` macro that includes both `Assistant(_)` and `System(_)` variants:
```rust
let assistant_count = messages
    .iter()
    .filter(|m| {
        matches!(
            m,
            ChatCompletionRequestMessage::Assistant(_) | ChatCompletionRequestMessage::System(_)
        )
    })
    .count();
```

When a previous compression injected a summary system message into the conversation body, subsequent compressions will count that system message as an "assistant" message. The summary text "N earlier messages (X user, Y assistant)" will report an inflated assistant count (Y = actual_assistant + system_messages). The total N is from `messages.len()` which includes all message types, so the discrepancy is visible: X + Y != N when system messages are present.

**Fix:** Either count system messages separately or exclude them from the assistant count:
```rust
let assistant_count = messages
    .iter()
    .filter(|m| matches!(m, ChatCompletionRequestMessage::Assistant(_)))
    .count();
let system_count = messages
    .iter()
    .filter(|m| matches!(m, ChatCompletionRequestMessage::System(_)))
    .count();
// Then use both in the summary format string.
```

### WR-03: compress_context aggressive fallback (token-count branch) lacks verification that the fallback output actually reduces token count

**File:** `crates/jadepaw-agent/src/window.rs:162-176`
**Issue:** The token-count verification path (lines 161-176) constructs an aggressive fallback when `compressed_tokens >= original_tokens`, but the fallback output itself is NOT verified to reduce the token count. If the fallback messages `[summary_msg, recent_n_turns...]` still exceed 65% of the context window (theoretically possible if recent N turns contain enormous tool outputs), the function returns them without further reduction.

On the next ReAct loop iteration, `should_compress()` will return `true` again, and `compress_context` will be called on the already-fallback-processed messages. The early-return guard at line 97 (`messages.len() <= n_msgs_to_keep + 3`) will fire (since the fallback output has at most `1 + 2*recent_n` messages), causing the function to return unchanged messages. This creates a persistent re-compression cycle that cannot make progress because the early-return guard prevents any further action.

The practical risk is low: to trigger this, 11 messages (with `recent_n=5`) would need to exceed ~83,200 tokens (65% of GPT-4o's 128K context window), requiring each message to average ~7,500 tokens. But it is a correctness gap in the compression algorithm.

**Fix:** Add a guard against the degenerate case in the aggressive fallback itself:
```rust
let fallback_tokens = count_tokens(&fallback, model);
if fallback_tokens >= original_tokens {
    // Even aggressive fallback cannot compress. Truncate to last N turns only,
    // dropping the summary to guarantee progress.
    let mut minimal = Vec::with_capacity(recent_len);
    let split = body.len().saturating_sub(n_msgs_to_keep);
    minimal.extend(body[split..].iter().cloned());
    return minimal;
}
```

### WR-04: run_agent passes TenantId::default() (random UUID) instead of deriving from authenticated request context

**File:** `crates/jadepaw-agent/src/lib.rs:125`
**Issue:** In `run_agent()`, the tenant_id is set to `TenantId::default()` which creates a new random UUID v7 on every call:
```rust
r#loop::react_loop(
    ...
    TenantId::default(),    // line 125
    ...
)
```

In the current code path, `session_repo` is `None` for fresh sessions, so this random UUID is never persisted and the isolation concern is moot. However, it represents a design gap:
- The tenant_id should come from the authenticated request context (e.g., `AgentRequest` should carry a `tenant_id` field, or it should be injected by middleware/auth layer)
- If fresh-session persistence is enabled in a future iteration (by passing `Some(repo)`), the random UUID would create un-recoverable sessions (no way to look up by tenant)
- The `resume_session` path correctly receives `tenant_id` as a parameter, creating an inconsistency in how tenant context flows through the two entry points

**Fix:** Add a `tenant_id: TenantId` field to `AgentRequest` in `jadepaw-core`, or add a `tenant_id` parameter to `run_agent()`:
```rust
pub async fn run_agent(
    req: AgentRequest,
    tenant_id: TenantId,  // new parameter
    pool: Arc<InstancePool>,
    ...
)
```

### WR-05: save() cross-tenant collision error message omits the tenant_id that triggered the collision

**File:** `crates/jadepaw-db/src/sqlite_repo.rs:122-127`
**Issue:** When the upsert's WHERE clause filters out the row due to `tenant_id` mismatch, the error message only includes `session_id`:
```rust
anyhow::bail!(
    "cross-tenant session_id collision: session {} exists under a different tenant",
    session_id
);
```

For operational debugging of a cross-tenant collision (which, while cryptographically infeasible for UUID v7, could indicate a UUID reuse bug or external injection), knowing WHICH tenant triggered the collision and WHICH tenant owns the existing session is essential for root cause analysis. The `tenant_id` used in the save call is available in scope but not included in the error message.

**Fix:** Include both tenant IDs:
```rust
anyhow::bail!(
    "cross-tenant session_id collision: session {} belongs to a different tenant (attempted insert with tenant {})",
    session_id,
    tenant_id
);
```

### WR-06: resume_session user_message parameter passed as empty string; if pre_existing_messages is empty (corrupted snapshot), build_initial_messages produces degenerate output

**File:** `crates/jadepaw-agent/src/lib.rs:294`
**Issue:** In `resume_session`, the `user_message` parameter to `react_loop` is hardcoded to `""` with the comment "not used when pre_existing_messages is not empty." This is correct in normal operation. However, if `pre_existing_messages` is somehow empty (snapshot corruption, deserialization producing zero messages, manual DB manipulation), `react_loop` at line 154-156 will call:
```rust
llm::build_initial_messages(system_prompt, user_message, context)
```
with `user_message = ""` and `context = None` (also hardcoded at line 295). This produces `[system("..."), user("")]` — a degenerate initial message list with an empty user message. The LLM may produce arbitrary output or error in response to an empty user message.

**Fix:** Add a defensive check and return an error if pre_existing_messages is unexpectedly empty:
```rust
if pre_existing_messages.is_empty() {
    return Err(JadepawError::agent_terminated(
        jadepaw_core::AgentTerminationReason::InfrastructureError {
            reason: "corrupted session snapshot: pre_existing_messages is empty".to_string(),
            turn: 0,
        },
    ));
}
```

## Info

### IN-01: migrations.rs comment references wrong migration path

**File:** `crates/jadepaw-db/src/migrations.rs:18`
**Issue:** The comment states `sqlx::migrate!("../migrations")` but the actual code in `sqlite_repo.rs:62` uses `sqlx::migrate!("./migrations")`. The `../migrations` path would resolve to `crates/migrations/` and fail at compile time. The code is correct — only the comment is stale. This was flagged as IN-02 in the previous review and was not fixed (Info-level findings were out of scope for the fix iteration).

**Fix:**
```rust
// Migrations are embedded via sqlx::migrate!("./migrations") in sqlite_repo.rs.
```

### IN-02: GuardConfig.recent_turns has redundant public field and public getter method

**File:** `crates/jadepaw-agent/src/guard.rs:32,48-50`
**Issue:** The field `pub recent_turns: u32` (line 32) and the method `pub fn recent_turns(&self) -> u32` (line 48) are both public and have the same name. The method is a trivial getter returning `self.recent_turns`. Rust allows this (methods shadow fields in method-call position), but having both is redundant. Callers at `loop.rs:178` use `guard_config.recent_turns()` (method syntax), but field access `guard_config.recent_turns` would be equivalent given the field is public. This was flagged as IN-03 in the previous review and was not fixed.

**Fix:** Either remove the method (keep field access) or make the field private (keep method for encapsulation). The field needs to remain accessible for `serde` deserialization, which works with private fields.

### IN-03: SessionSnapshot.termination_reason_json is always None in write paths

**File:** `crates/jadepaw-agent/src/loop.rs:345`, `crates/jadepaw-db/src/models.rs:77`
**Issue:** The `SessionSnapshot::termination_reason_json` field is defined in the data model, present in the SQL schema (as nullable TEXT), and properly read back in `load()` and `list_by_tenant()`. However, all write paths set it to `None`:
- `react_loop` checkpoint construction at line 345: `termination_reason_json: None`
- Test helpers in `session_persistence.rs` also set it to `None`

The field is never populated with an actual `AgentTerminationReason` when a session ends. This means the `SessionSummary::termination_reason` field (now correctly typed as `Option<AgentTerminationReason>` after WR-05 fix) will always be `None` for all sessions, regardless of whether they ended normally or were terminated by a guard.

**Fix:** When the agent loop terminates or the guard fires, construct the appropriate `AgentTerminationReason`, serialize it, and persist it via `update_status()` or a final `save()` call. The infrastructure for this already exists (the field, the schema, the deserialization) and only the write path is missing.

### IN-04: compress_context token-count fallback duplicates message-count fallback logic

**File:** `crates/jadepaw-agent/src/window.rs:147-155, 170-176`
**Issue:** The aggressive fallback logic is duplicated verbatim: once for the message-count reduction check (lines 147-155) and again for the token-count reduction check (lines 170-176). Both blocks construct `[summary_msg, recent_n_turns...]` using the same algorithm. The `recent_len` variable is computed at line 124 but the fallback at line 170 uses it without re-computing (it's still in scope), which is correct but fragile if the code is restructured.

**Fix:** Extract the aggressive fallback into a private helper function:
```rust
fn aggressive_fallback(
    messages: &[ChatCompletionRequestMessage],
    summary_msg: ChatCompletionRequestMessage,
    n_msgs_to_keep: usize,
) -> Vec<ChatCompletionRequestMessage> {
    let body = &messages[2..];
    let split = body.len().saturating_sub(n_msgs_to_keep);
    let mut fallback = Vec::with_capacity(1 + body.len() - split);
    fallback.push(summary_msg);
    fallback.extend(body[split..].iter().cloned());
    fallback
}
```

---

_Reviewed: 2026-06-05_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_