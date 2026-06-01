---
phase: 03-agent-runtime
plan: 01
subsystem: agent-runtime
type: execute
wave: 1
status: complete
tags: [agent-types, react-loop, termination-guard, core-types]
requires: []
provides:
  - agent_types.rs (AgentRequest, AgentResponse, ReActStep, AgentTerminationReason)
  - guest_exports.rs (GuestExports trait, ToolDef, NextAction, ToolChoice)
  - loop.rs (LoopConfig, LlmProvider trait, react_loop)
  - guard.rs (GuardConfig, run_with_guard)
  - run_agent() entry point composing guard + loop
affects:
  - jadepaw-core (new: agent_types, guest_exports; mod: error, lib)
  - jadepaw-agent (new: loop, guard; mod: lib, Cargo.toml)
tech-stack:
  added:
    - serde_json (jadepaw-core dep, for ReActStep args typed as serde_json::Value)
    - anyhow 1.0 (jadepaw-agent dep, internal error handling)
    - async-trait 0.1 (jadepaw-agent dep, LlmProvider trait)
  patterns:
    - Additive-only trait pattern (GuestExports mirrors HostFunctions in jadepaw-core)
    - tokio::select! guard pattern (run_with_guard races loop future vs wall-clock timeout)
    - Optional guest override with LLM fallback (all GuestExports methods return Option<...>)
    - Per-turn fuel reset (1_000_000 units, Pitfall 3 prevention)
    - mpsc channel for real-time ReActStep streaming (capacity 256)
completed: "2026-06-01T04:10:50Z"
duration: 816s
started: "2026-06-01T03:57:13Z"
tasks:
  1: { status: complete, commit: 169ed63 }
  2: { status: complete, commit: 359e860 }
  3: { status: complete, commit: e4c6217 }
key-decisions:
  - AgentTerminationReason uses u64 for time values instead of Duration to maintain PartialEq/Eq derive compatibility on JadepawError
  - GuestExports methods return Option<...> defaulting to None for LLM-fallback behavior, following additive-only pattern
  - run_with_guard uses FnOnce() -> Fut closure pattern to avoid lifetime issues with borrowed SessionHandle
  - react_loop currently takes a single turn then finishes (Plan 02 adds multi-turn with real LLM streaming)
  - Cargo.lock was committed alongside each task to ensure reproducible builds
key-files:
  created:
    - crates/jadepaw-core/src/agent_types.rs (AgentRequest, AgentResponse, ReActStep, AgentTerminationReason)
    - crates/jadepaw-core/src/guest_exports.rs (GuestExports trait, ToolDef, NextAction, ToolChoice)
    - crates/jadepaw-agent/src/loop.rs (LoopConfig, LlmProvider, react_loop)
    - crates/jadepaw-agent/src/guard.rs (GuardConfig, run_with_guard)
    - crates/jadepaw-core/tests/agent_types.rs (14 tests)
    - crates/jadepaw-agent/tests/agent_loop.rs (4 tests)
    - crates/jadepaw-agent/tests/termination.rs (5 tests)
  modified:
    - crates/jadepaw-core/src/error.rs (AgentTerminated variant + constructor + Display)
    - crates/jadepaw-core/src/lib.rs (new module declarations + re-exports)
    - crates/jadepaw-core/Cargo.toml (serde_json dep)
    - crates/jadepaw-agent/src/lib.rs (run_agent entry point + module declarations + re-exports)
    - crates/jadepaw-agent/Cargo.toml (anyhow, async-trait, wat, tempfile deps)
---

# Phase 3 Plan 1: Core Types, ReAct Loop Skeleton, and Termination Guards

Established the agent runtime type system, ReAct loop skeleton with mocked LLM integration, and termination protection guards. After this plan, `run_agent()` is callable programmatically -- it accepts an `AgentRequest`, orchestrates think-act-observe cycles, enforces safety limits, and returns a structured `AgentResponse`.

## Plan Execution

All three tasks completed successfully with zero test regressions across the full workspace.

### Deviations from Plan

None -- plan executed exactly as written. All three tasks delivered the specified files, types, and behaviors.

### Auto-fixed Issues

[Rule 1 - Bug] `set_fuel` context() incompatibility with wasmtime Error type. wasmtime's internal Error doesn't implement `std::error::Error`, so `anyhow::Context` cannot be used. Fixed by replacing `.context()` with `.map_err()` in loop.rs.

[Rule 1 - Bug] `#[async_trait]` trait import missing from jadepaw-agent. The `LlmProvider` trait uses `async_trait::async_trait` but `async-trait` wasn't in jadepaw-agent's dependencies. Fixed by adding `async-trait = "0.1"` to Cargo.toml.

[Rule 3 - Blocking] Double name import conflict in lib.rs. Both private `use guard::GuardConfig` and public `pub use guard::GuardConfig` brought `GuardConfig` into scope twice. Fixed by removing private use imports and using fully-qualified paths (`guard::GuardConfig`) inside `run_agent`.

## Verification

All success criteria met:

- [x] `run_agent()` compiles and can be called programmatically with an `AgentRequest`
- [x] ReAct loop skeleton iterates with per-turn fuel reset, producing thought and finished steps
- [x] Termination guard enforces both iteration limit (in-loop check) and wall-clock timeout (tokio::select!)
- [x] All core types (AgentRequest, AgentResponse, ReActStep, AgentTerminationReason) in jadepaw-core with serde support
- [x] GuestExports trait provides optional decision-point interface with LLM-fallback defaults
- [x] Tests verify: serde roundtrips, loop produces structured trace, guard terminates on timeout

Test coverage summary:
- **agent_types.rs**: 14 tests (serde roundtrips for all types and ReActStep variants, defaults, Display implementations, error wrapping)
- **agent_loop.rs**: 4 tests (thought+finished trace, LLM failure, structured response, guard composition with timeout)
- **termination.rs**: 5 tests (config defaults, normal completion, timeout, timeout value propagation, error-to-WasmTrap mapping)
- **Existing tests**: 20+ tests from prior phases pass with zero regressions

## Threat Flags

None. All threat dispositions from the plan's `<threat_model>` are addressed:
- T-03-01 (prompt injection): accepted, documented as known limitation
- T-03-02 (DoS infinite loop): mitigated via run_with_guard tokio::select! + max_iterations
- T-03-03 (timeout bypass): mitigated -- sleep future fires unconditionally, no code path can extend

## Known Stubs

None. All types, functions, and guards are fully wired. The LLM integration is intentionally mocked via the `LlmProvider` trait -- Plan 02 replaces this with real async-openai streaming.