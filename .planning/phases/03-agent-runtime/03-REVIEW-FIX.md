---
phase: 03-agent-runtime
fixed_at: 2026-06-01T00:00:00Z
review_path: .planning/phases/03-agent-runtime/03-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 9
skipped: 0
status: all_fixed
---

# Phase 3: Code Review Fix Report

**Fixed at:** 2026-06-01
**Source review:** .planning/phases/03-agent-runtime/03-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 9 (4 Critical + 5 Warning)
- Fixed: 9
- Skipped: 0

## Fixed Issues

### CR-01: run_with_guard 将所有 loop 错误统一映射为 WasmTrap -- 语义错误

**Files modified:** `crates/jadepaw-agent/src/guard.rs`
**Commit:** 1416c58
**Applied fix:** Replaced the blanket `WasmTrap` mapping with error classification based on the anyhow error message content. "max iterations" errors now map to `MaxIterationsReached` with correct iter/max values. "LLM call failed" and "output channel closed" errors get descriptive WasmTrap reasons. Added `extract_turn_from_error()` helper to parse turn numbers from structured error messages.

### CR-02: react_loop 每 turn 对整个 LLM 响应流都发 Thought 事件

**Files modified:** `crates/jadepaw-agent/src/llm.rs`, `crates/jadepaw-agent/src/loop.rs`
**Commit:** 9b1150d
**Applied fix:** `stream_llm_response` no longer emits per-token Thought events. Instead, it only accumulates tokens into a single string. `react_loop` now emits a single `ReActStep::Thought` event with the complete LLM response after streaming completes. Channel close detection uses `tx.is_closed()` for graceful early termination.

### CR-03: llm::NextAction 与 guest_exports::NextAction 重复定义

**Files modified:** `crates/jadepaw-agent/src/llm.rs`, `crates/jadepaw-agent/src/loop.rs`
**Commit:** 7341408
**Applied fix:** Renamed `llm::NextAction` to `LlmDirective` with documentation clarifying it is distinct from `guest_exports::NextAction`. Updated all references in `llm.rs`, `loop.rs`, and tests.

### CR-04: parse_next_action 未解析完整的 LLM 响应结构 -- THOUGHT 内容丢失

**Files modified:** `crates/jadepaw-agent/src/llm.rs`, `crates/jadepaw-agent/src/loop.rs`
**Commit:** e68138d
**Applied fix:** Added `thought: String` field to all `LlmDirective` variants (Act, Finish, ContinueThinking). Implemented `extract_thought()` helper that parses the THOUGHT: prefix from LLM responses. Updated `react_loop` to destructure the new thought field. All 8 unit tests updated and pass.

### WR-01: JadepawError 未实现 std::error::Error::source()

**Files modified:** `crates/jadepaw-core/src/error.rs`
**Commit:** f13856c
**Applied fix:** Replaced the default `impl std::error::Error for JadepawError {}` with an explicit implementation that documents the design decision (variants are self-contained with string descriptions, no chained source errors). Includes a comment indicating where future variants should add source() arms.

### WR-02: run_with_guard 中映射错误时丢失 turn 信息

**Files modified:** `crates/jadepaw-agent/src/guard.rs`
**Commit:** 1416c58 (combined with CR-01)
**Applied fix:** Added `extract_turn_from_error()` function that parses the "on turn N" pattern from loop error messages. The extracted turn number is used in all termination reason variants instead of the hardcoded 0.

### WR-03: run_agent 中 trace 缺少 Finished 值时 unwrap_or_default 返回空字符串

**Files modified:** `crates/jadepaw-agent/src/lib.rs`
**Commit:** d792887
**Applied fix:** Replaced `unwrap_or_default()` with `ok_or_else()` that produces an `AgentTerminated` error when no `Finished` step is found in the trace. This lets callers distinguish between "agent completed with empty answer" and "agent was terminated without producing a final answer".

### WR-04: SSE injection test 被弱化 -- 仅验证内容存在但不验证安全性

**Files modified:** `crates/jadepaw-agent/tests/sse_streaming.rs`
**Commit:** 5bc2c65
**Applied fix:** Added assertions to verify that no injected event declarations appear after newlines in the SSE output (`\nevent: ` count should be 0). The test now confirms the axum Event builder properly encodes injected SSE control characters as `data:` fields rather than allowing them to become new event declarations.

### WR-05: react_loop 中 Action step 的 args JSON 解析使用 unwrap_or 作为兜底

**Files modified:** `crates/jadepaw-agent/src/loop.rs`, `crates/jadepaw-agent/Cargo.toml`
**Commit:** 2c336f1
**Applied fix:** Replaced silent `unwrap_or` fallback with an explicit `match` that logs a `tracing::warn!` message when LLM-produced args cannot be parsed as JSON. The fallback to `Value::String` is preserved for robustness. Added `tracing = "0.1"` dependency to jadepaw-agent.

---

_Fixed: 2026-06-01T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_