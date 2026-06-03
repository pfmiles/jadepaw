---
phase: 04-tool-system
plan: 01
subsystem: tool-system
tags: [tool-abstraction, tool-registry, capability-gating, mcp-compatible, agent-runtime]
requires: [Phase 2 capability types, Phase 3 ReAct loop types]
provides: [Tool trait, ToolResult enum, ToolDefinition struct, ToolRegistry with capability enforcement]
affects: [jadepaw-core, jadepaw-agent]
tech-stack:
  added: [dashmap (jadepaw-agent direct dep), async-trait (jadepaw-agent dev-dep)]
  patterns:
    - "Types in core (tool.rs), dispatch in agent (tool_registry.rs)"
    - "DashMap-based concurrent dispatch matching InstancePool pattern"
    - "Capability gate (can_call_tool) as authoritative policy decision point"
key-files:
  created:
    - crates/jadepaw-core/src/tool.rs
    - crates/jadepaw-agent/src/tool_registry.rs
  modified:
    - crates/jadepaw-core/src/agent_types.rs
    - crates/jadepaw-core/src/host_functions.rs
    - crates/jadepaw-core/src/lib.rs
    - crates/jadepaw-agent/src/lib.rs
    - crates/jadepaw-agent/Cargo.toml
    - crates/jadepaw-agent/src/loop.rs
    - crates/jadepaw-agent/src/stream.rs
    - crates/jadepaw-core/tests/agent_types.rs
    - crates/jadepaw-core/tests/host_functions.rs
    - crates/jadepaw-agent/tests/sse_streaming.rs
decisions:
  - "Tool type system (Tool trait + ToolResult + ToolDefinition) in jadepaw-core per D-01/D-04"
  - "ToolRegistry in jadepaw-agent with DashMap-backed concurrent dispatch per D-01/D-02"
  - "is_error field on Observation with #[serde(default)] for Phase 3 backward compatibility"
  - "http_request method on HostFunctions trait (additive-only per D-01 constraints)"
duration: ~8m
completed_date: 2026-06-03
---

# Phase 4 Plan 1: Tool Abstraction Layer Summary

Defined the Tool trait, ToolResult enum, and ToolDefinition struct in jadepaw-core, implemented the ToolRegistry dispatch infrastructure in jadepaw-agent with DashMap-backed concurrent storage and capability-aware authorization.

## Tasks

| # | Name | Status | Commit |
|---|------|--------|--------|
| 1 | Create Tool trait + ToolResult + ToolDefinition in jadepaw-core | Complete | 33ae283 |
| 2 | Add is_error to ReActStep::Observation + http_request to HostFunctions | Complete | 2f2ae0e |
| 3 | Implement ToolRegistry in jadepaw-agent with capability gating | Complete | fcf7083 |

## Verification

### Plan-Level Checks
- `cargo build -p jadepaw-core` -- PASS
- `cargo build -p jadepaw-agent` -- PASS
- `cargo test -p jadepaw-core` -- PASS (25 tests)
- `cargo test -p jadepaw-agent` -- PASS (35 tests: 21 unit + 14 integration)
- `cargo clippy -p jadepaw-agent -- -D warnings` -- BLOCKED by pre-existing clippy errors in jadepaw-core (out-of-scope)

### Acceptance Criteria
1. Tool trait, ToolResult enum, ToolDefinition struct in jadepaw-core -- COMPLETE
2. ReActStep::Observation.is_error with #[serde(default)] -- COMPLETE
3. HostFunctions::http_request() method signature -- COMPLETE
4. ToolRegistry with DashMap, MCP-compatible list/call, capability gating -- COMPLETE
5. Both crates build and pass existing tests -- COMPLETE

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] is_error field missing in test and production Observation initializers**
- **Found during:** Task 3
- **Issue:** Adding `is_error: bool` field to `ReActStep::Observation` caused compile errors in: loop.rs placeholder, stream.rs pattern match + test initializers, sse_streaming.rs integration tests
- **Fix:** Added `is_error: false` to all Observation initializers, used `..` rest pattern in stream.rs match
- **Files modified:** crates/jadepaw-agent/src/loop.rs, crates/jadepaw-agent/src/stream.rs, crates/jadepaw-agent/tests/sse_streaming.rs
- **Commit:** fcf7083

**2. [Rule 1 - Bug] TestHostFn missing http_request implementation**
- **Found during:** Task 2
- **Issue:** Adding `http_request` to `HostFunctions` trait broke the test implementor `TestHostFn` in core tests
- **Fix:** Added `http_request` stub returning `CapabilityDenied` to match existing pattern
- **Files modified:** crates/jadepaw-core/tests/host_functions.rs
- **Commit:** 2f2ae0e

**3. [Rule 3 - Blocking] async-trait crate not available in jadepaw-agent test code**
- **Found during:** Task 3
- **Issue:** `#[async_trait]` needed in `tool_registry.rs` test module for EchoTool implementation of Tool trait, but not in jadepaw-agent dependencies
- **Fix:** Added `async-trait = "0.1"` to jadepaw-agent dev-dependencies
- **Files modified:** crates/jadepaw-agent/Cargo.toml
- **Commit:** fcf7083

### Out-of-Scope Issues

- **clippy::derivable_impls in jadepaw-core**: Two pre-existing clippy errors in `agent_types.rs` and `guest_exports.rs` (not modified by this plan) block `-D warnings` on jadepaw-agent since core is a transitive dependency. Logged to `deferred-items.md`.

## Decisions Made

1. **Tool type system in jadepaw-core**: Tool trait, ToolResult enum, ToolDefinition struct all live in jadepaw-core with zero internal dependencies. Enables all crates to reference them without pulling in wasmtime.

2. **ToolRegistry with DashMap**: Lock-free concurrent reads, same pattern as Phase 2 InstancePool. Name-to-ID index provides O(1) lookup.

3. **is_error backward compatibility**: `#[serde(default)]` on Observation's new field ensures Phase 3 traces deserialize correctly.

4. **http_request additive-only**: HostFunctions trait gains http_request without modifying existing methods, per D-01 constraints.

## Threat Flags

None. The threat model's mitigations (T-04-01 unknown tool rejection, T-04-02 capability gate, T-04-03 deferred arg validation) are properly implemented.

## Known Stubs

None. No placeholder values, TODO markers, or unwired data sources in produced code.

## Tool Impl Dependencies (for Plan 02)

The following items are stubs that Plan 02 must fill in:
- `jadepaw-wasm` implementor of `HostFunctions` needs `http_request` (currently only the trait defines it)
- Tool implementations (FileReadTool, FileWriteTool, HttpRequestTool) in jadepaw-wasm not yet created
- `react_loop()` still uses a placeholder Observation -- Plan 03 will wire it to `ToolRegistry::call_tool()`

These are documented as cross-plan dependencies, not execution stubs.