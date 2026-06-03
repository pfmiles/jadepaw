---
phase: 04-tool-system
plan: 03
subsystem: agent-runtime
tags: [react-loop, tool-registry, sse, observation, is-error]
requires: [04-01, 04-02]
provides: ["Real tool dispatch from ReAct loop via ToolRegistry",
           "Augmented system prompt with tool descriptions",
           "SSE observation events with is_error JSON field"]
affects: [jadepaw-agent, jadepaw-core]
tech-stack:
  added: []
  patterns:
    - "ToolRegistry dispatch replaces placeholder observation in LlmDirective::Act branch"
    - "Arc<ToolRegistry> shared across sessions via Option<Arc<ToolRegistry>> parameter"
    - "build_system_prompt_with_tools() injects MCP tools/list format into system prompt"
    - "Observation SSE events upgraded from plain text to JSON {result, is_error}"
key-files:
  created: []
  modified:
    - crates/jadepaw-agent/src/loop.rs
    - crates/jadepaw-agent/src/lib.rs
    - crates/jadepaw-agent/src/llm.rs
    - crates/jadepaw-agent/src/stream.rs
    - crates/jadepaw-agent/tests/agent_loop.rs
decisions:
  - "ToolRegistry dispatch uses parsed_args.clone() to avoid move-after-use in Action + then call_tool"
  - "Observation result_str cloned before being consumed by ReActStep struct"
  - "Empty ToolRegistry is created as default when None is passed (backward compatible with Phase 3)"
  - "System prompt augmentation is conditional: only applied when tool_definitions is non-empty"
  - "SSE observation payload uses serde_json json! macro with tracing::error! fallback on serialization failure"
metrics:
  duration: 678s
  completed_date: "2026-06-03T11:21:58Z"
---

# Phase 4 Plan 3: ReAct Loop Integration Summary

Tool registry dispatch wired into the ReAct loop, replacing the Phase 3 placeholder observation with real `ToolRegistry::call_tool()` execution. System prompt augmented with tool descriptions in MCP format. SSE observation events upgraded to JSON with `is_error` discrimination.

## Execution Summary

| Task | Name | Commit | Status |
|------|------|--------|--------|
| 1 | Wire ToolRegistry into react_loop() | 9558855 | Passed |
| 2 | Wire ToolRegistry into run_agent() + augment system prompt | 0202bb7 | Passed |
| 3 | Update SSE Observation event with is_error field | db28a2b | Passed |

### Task 1: ToolRegistry dispatch in react_loop()

**Files:** `crates/jadepaw-agent/src/loop.rs`, `crates/jadepaw-agent/src/lib.rs`

- Added `tool_registry: &ToolRegistry` parameter to `react_loop()`
- Replaced placeholder observation (lines 219-228) with `tool_registry.call_tool(&tool, parsed_args, session).await`
- Constructed `ReActStep::Observation { result_str, is_error }` from ToolResult
- Appended tool result as `ChatCompletionRequestUserMessage` to LLM message history
- Removed "Full tool execution is coming in Phase 4" text — grep returns 0 matches
- Added `ChatCompletionRequestUserMessage` to async-openai imports
- Fixed move-after-use: cloned `parsed_args` before Action step
- Updated doc comment to describe real tool dispatch

### Task 2: run_agent() wiring + system prompt augmentation

**Files:** `crates/jadepaw-agent/src/lib.rs`, `crates/jadepaw-agent/src/llm.rs`, `crates/jadepaw-agent/tests/agent_loop.rs`

- Added `tool_registry: Option<Arc<ToolRegistry>>` parameter to `run_agent()`
- Created empty ToolRegistry when None is passed (backward compatible)
- Added `build_system_prompt_with_tools()` in llm.rs — injects MCP tools/list format
- Conditional augmentation: only when `tool_definitions.is_empty() == false`
- Passed `augmented_prompt` and `&registry` to `react_loop()`
- Re-exported `build_system_prompt_with_tools` from lib.rs
- Updated test callers to pass `None` for tool_registry
- Added `ToolDefinition` import to llm.rs

### Task 3: SSE Observation is_error

**Files:** `crates/jadepaw-agent/src/stream.rs`

- Upgraded observation event from plain text to JSON `{"result": "...", "is_error": bool}`
- Destructured `is_error` from `ReActStep::Observation` in SSE match arm
- Added serialization error fallback with `tracing::error!` log
- Updated doc comment table to reflect JSON format
- All existing tests pass unchanged (Observation constructors already had `is_error` from Plan 01)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed move-after-use of parsed_args in loop.rs**
- **Found during:** Task 1
- **Issue:** `parsed_args` was moved into `ReActStep::Action { args }` then used again for `call_tool()`
- **Fix:** Added `.clone()` on `parsed_args` when constructing Action step
- **Files modified:** `crates/jadepaw-agent/src/loop.rs`
- **Commit:** 9558855

**2. [Rule 1 - Bug] Fixed move-after-use of result_str in loop.rs**
- **Found during:** Task 1
- **Issue:** `result_str` was moved into `ReActStep::Observation { result }` then used in observation_msg format string
- **Fix:** Added `.clone()` on `result_str` when constructing Observation
- **Files modified:** `crates/jadepaw-agent/src/loop.rs`
- **Commit:** 9558855

**3. [Rule 3 - Blocking] Updated test callers for new run_agent() signature**
- **Found during:** Task 2
- **Issue:** `tests/agent_loop.rs` called `run_agent()` with 4 args but new signature requires 5 (tool_registry parameter)
- **Fix:** Added `None` as tool_registry argument in test caller
- **Files modified:** `crates/jadepaw-agent/tests/agent_loop.rs`
- **Commit:** 0202bb7

### Pre-existing Issues (Out-of-Scope)

- **Clippy warnings in jadepaw-core:** `agent_types.rs` (derivable_impls on AgentRequest Default) and `guest_exports.rs` (derivable_impls on ToolChoice Default) are pre-existing lint warnings in files NOT modified by this plan. These block `cargo clippy -p jadepaw-agent -- -D warnings` at the dependency level but are not introduced by our changes. Logged to `deferred-items.md`.

### T-04-SC Supply Chain

No new dependencies added. All types used (`Tool`, `ToolResult`, `ToolRegistry`) are from Phase 01/02 already in the dependency tree. Supply chain threat accepted per plan.

## Verification

| Criterion | Result |
|-----------|--------|
| Placeholder removed from loop.rs | PASS (grep returns 0) |
| call_tool in loop.rs >= 1 | PASS (2 occurrences) |
| is_error in stream.rs >= 3 | PASS (6 occurrences) |
| "is_error" in stream.rs >= 2 | PASS (3 occurrences) |
| cargo build -p jadepaw-agent | PASS |
| cargo build --workspace | PASS |
| cargo test -p jadepaw-agent | PASS (35 tests, 0 failures) |
| cargo test --workspace | PASS (all crates, 0 failures) |

## Known Issues

- **Pre-existing clippy warnings in jadepaw-core** (`agent_types.rs`, `guest_exports.rs`) block `cargo clippy -p jadepaw-agent -- -D warnings`. These are NOT introduced by this plan and are deferred to a separate cleanup task.

## Self-Check: PASSED

- [x] SUMMARY.md written to `.planning/phases/04-tool-system/04-03-SUMMARY.md`
- [x] All 3 commits verified: 9558855, 0202bb7, db28a2b
- [x] All modified files exist and are tracked
- [x] Workspace builds and all tests pass
- [x] No untracked files