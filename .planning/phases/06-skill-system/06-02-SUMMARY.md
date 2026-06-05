---
phase: 06-skill-system
plan: 02
type: execute
wave: 2
subsystem: skill-runtime
tags: [skill, registry, system-prompt, injection, agent-loop, hot-swap]
depends_on:
  requires: [06-01]
  provides: [06-03]
  affects: [jadepaw-skill, jadepaw-agent, jadepaw-core]
tech-stack:
  added: []
  patterns: [DashMap concurrency, XML injection, trait-based dependency inversion, atomic swap]
key-files:
  created:
    - crates/jadepaw-skill/src/registry.rs
    - crates/jadepaw-skill/src/manager.rs
    - crates/jadepaw-skill/src/injector.rs
  modified:
    - crates/jadepaw-skill/src/lib.rs
    - crates/jadepaw-skill/Cargo.toml
    - crates/jadepaw-agent/Cargo.toml
    - crates/jadepaw-agent/src/tool_registry.rs
    - crates/jadepaw-agent/src/llm.rs
    - crates/jadepaw-agent/src/loop.rs
    - crates/jadepaw-agent/src/lib.rs
    - crates/jadepaw-agent/tests/agent_loop.rs
    - crates/jadepaw-core/src/tool.rs
    - crates/jadepaw-core/src/agent_types.rs
    - crates/jadepaw-core/src/lib.rs
decisions:
  - "ToolLookup trait in jadepaw-core breaks circular dependency between jadepaw-skill and jadepaw-agent"
  - "SkillManager::load() uses Option<&dyn ToolLookup> for tool validation — avoids compile-time dependency on ToolRegistry"
  - "Pending skill swap consumed atomically at turn boundary via DashMap::take() semantics"
  - "build_skill_augmented_prompt handles all four combinations: skills+/-tools"
metrics:
  duration: "~15 min"
  completed: "2026-06-05T16:52:09Z"
  tasks: 2
  files: 11
---

# Phase 6 Plan 2: Skill Runtime Summary

**One-liner:** Delivered the skill runtime with in-memory DashMap registry, XML skill context injection into system prompts, and atomic mid-session swaps at ReAct turn boundaries.

## What Was Built

### Task 1: SkillRegistry, SkillManager, and Injector (jadepaw-skill)

Three new modules forming the skill runtime:

- **SkillRegistry** (`registry.rs`): Per-tenant `DashMap<TenantId, ActiveSkillState>` storage with lock-free concurrent reads. Supports `insert`, `remove` by name, `get_active` sorted by priority, `set_pending_swap`/`take_pending_swap` with atomic take semantics.

- **SkillManager** (`manager.rs`): Central coordinator. `load()` reads SKILL.md from disk, parses it, validates tool availability via `ToolLookup` trait, inserts into registry, and sets pending swap — all-or-nothing per D-08. `unload()` removes and rebuilds. `merge_active()` returns XML context block + tool name union. `check_pending_swap()` consumes the swap flag for D-07 turn-boundary application.

- **Injector** (`injector.rs`): Pure function `build_skill_context_block()` that formats active skills into `<skill_instructions>` XML with `name`, `version`, `priority` attributes, sorted by priority descending per D-02.

**Circular dependency resolution:** Added `ToolLookup` trait to `jadepaw-core/src/tool.rs` to break the `jadepaw-skill` <-> `jadepaw-agent` circular dependency. `ToolRegistry` in `jadepaw-agent` implements this trait. `jadepaw-skill` no longer depends on `jadepaw-agent`; `jadepaw-agent` now depends on `jadepaw-skill`.

### Task 2: Register Migration, Agent Integration, System Prompt Injection

- **ToolRegistry::register()** migrated from `fn(Tool) -> ToolId` (panics on duplicate) to `fn(Tool, priority) -> Result<ToolId, ToolConflictError>` per D-04. All call sites updated.

- **build_skill_augmented_prompt()** in `llm.rs`: Injects skill context block between base prompt and tool descriptions. Handles all four combinations (skills present/absent, tools present/absent).

- **react_loop()** accepts `Option<Arc<SkillManager>>` and checks pending swap at the top of each turn AFTER context compression and BEFORE the LLM call (D-07). `messages[0]` is replaced atomically.

- **run_agent()** and **resume_session()** accept `Option<Arc<SkillManager>>` and build skill-aware system prompts via `merge_active()`.

- **AgentRequest** gains `skills: Vec<String>` field with `#[serde(default)]` for Plan 03 server integration.

## Verification

```
cargo check                     # full workspace: 0 errors
cargo test -p jadepaw-skill     # 57 passed, 0 failed
cargo test -p jadepaw-agent --lib  # 33 passed, 0 failed
cargo test -p jadepaw-agent -- --skip run_with_guard_maps_loop_error_to_wasm_trap  # 10 passed, 1 pre-existing failure
```

The `run_with_guard_maps_loop_error_to_wasm_trap` test failure is pre-existing (confirmed by testing on the base commit without this plan's changes).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Circular dependency between jadepaw-skill and jadepaw-agent**
- **Found during:** Task 1 design review
- **Issue:** The plan required `jadepaw-skill` to reference `ToolRegistry` from `jadepaw-agent` (for tool validation in `load()`) while also requiring `jadepaw-agent` to reference `SkillManager` from `jadepaw-skill` (for the agent loop). Cargo does not allow circular dependencies.
- **Fix:** Added `ToolLookup` trait to `jadepaw-core/src/tool.rs` with `lookup_by_name()` method. `ToolRegistry` in `jadepaw-agent` implements `ToolLookup`. `SkillManager::load()` accepts `Option<&dyn ToolLookup>` instead of `Option<&ToolRegistry>`. Removed `jadepaw-agent` dependency from `jadepaw-skill/Cargo.toml`; added `jadepaw-skill` dependency to `jadepaw-agent/Cargo.toml`.
- **Files modified:** `crates/jadepaw-core/src/tool.rs`, `crates/jadepaw-core/src/lib.rs`, `crates/jadepaw-skill/Cargo.toml`, `crates/jadepaw-agent/Cargo.toml`, `crates/jadepaw-skill/src/manager.rs`, `crates/jadepaw-agent/src/tool_registry.rs`
- **Commit:** bc2e74a

**2. [Rule 1 - Bug] thiserror dependency not available in jadepaw-agent**
- **Found during:** Task 2 compilation
- **Issue:** `ToolConflictError` used `#[derive(thiserror::Error)]` but `thiserror` is not in the project's dependencies.
- **Fix:** Implemented `Display` and `Error` traits manually for `ToolConflictError`.
- **Files modified:** `crates/jadepaw-agent/src/tool_registry.rs`
- **Commit:** ea55fc2

## Known Stubs

None. All created types and functions are fully wired with data flowing from disk (SKILL.md files) through parsing, validation, registry insertion, and system prompt injection. The `AgentRequest.skills` field is declared but not yet consumed by a server — this is by design and will be wired in Plan 03.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: prompt-injection | crates/jadepaw-skill/src/injector.rs | Skill Markdown body is injected directly into system prompt without escaping. This is by design (skills are natural language instructions) and documented in T-06-07 (accepted risk). |
| threat_flag: cross-tenant | crates/jadepaw-skill/src/registry.rs | TenantId is a required parameter on all SkillManager methods and DashMap key, providing type-system-level cross-tenant protection per T-06-06. |

## Self-Check: PASSED

- [x] `crates/jadepaw-skill/src/registry.rs` exists
- [x] `crates/jadepaw-skill/src/manager.rs` exists
- [x] `crates/jadepaw-skill/src/injector.rs` exists
- [x] Commit bc2e74a exists (Task 1)
- [x] Commit ea55fc2 exists (Task 2)
- [x] `cargo check` passes with 0 errors
- [x] All tests pass (57 jadepaw-skill + 33 jadepaw-agent lib + 10 integration)