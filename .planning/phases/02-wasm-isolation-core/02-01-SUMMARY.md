---
phase: 02-wasm-isolation-core
plan: 01
subsystem: wasm-runtime
tags: [wasmtime, resource-limiter, capability-system, engine-factory, session-state, security-foundation]
requires: []
provides: [HostFunctions, InstanceCapabilities, SessionState, EngineFactory, resource-limiters, epoch-ticker]
affects: [jadepaw-core, jadepaw-wasm]
tech-stack:
  added: [async-trait, anyhow, wat]
  patterns: [delegating-chain-resource-limiter, store-per-session, engineweak-epoch-ticker]
key-files:
  created:
    - crates/jadepaw-core/src/types.rs
    - crates/jadepaw-core/src/error.rs
    - crates/jadepaw-core/src/host_functions.rs
    - crates/jadepaw-core/src/capabilities.rs
    - crates/jadepaw-core/tests/types.rs
    - crates/jadepaw-core/tests/capabilities.rs
    - crates/jadepaw-wasm/src/engine.rs
    - crates/jadepaw-wasm/src/limits/mod.rs
    - crates/jadepaw-wasm/src/limits/instance_hard.rs
    - crates/jadepaw-wasm/src/limits/tenant_quota.rs
    - crates/jadepaw-wasm/src/session.rs
    - crates/jadepaw-wasm/src/epoch.rs
    - crates/jadepaw-wasm/tests/engine_smoke.rs
    - crates/jadepaw-wasm/tests/limits.rs
    - crates/jadepaw-wasm/tests/fixtures/noop.wat
  modified:
    - crates/jadepaw-core/Cargo.toml
    - crates/jadepaw-core/src/lib.rs
    - crates/jadepaw-wasm/Cargo.toml
    - crates/jadepaw-wasm/src/lib.rs
decisions:
  - "D-01: HostFunctions trait in jadepaw-core using async-trait crate"
  - "D-10/D-12: InstanceCapabilities with default-deny; max_memory_mb defaults to 64"
  - "D-07/D-08: Delegating chain ResourceLimiter — InstanceHardLimiter Err() for 64MB hard cap, TenantQuotaLimiter Ok(false) for aggregate budget"
  - "D-09: Engine config with Fuel+Epoch+PoolingAllocationConfig from Day 1"
  - "D-09a: Extensible delegating chain via ResourceLimiter trait composition"
  - "D-11: SessionState as Store<T> data with session_id, tenant_id, capabilities, SessionLimits"
  - "wasmtime 45.0 API: PoolingAllocationConfig (not PoolingAllocatorConfig), table_growing required, async_support deprecated, epoch_deadline_async_yield_and_update returns ()"
  - "wasmtime::Error constructed via std::io::Error wrapper for ResourceLimiter Err() returns"
  - "Epoch ticker: Engine::clone() for increment_epoch(), EngineWeak::upgrade() for liveness check"
duration: ~16min
completed_date: "2026-05-30"
---

# Phase 02 Plan 01: Wasm Security Foundation Summary

**One-liner:** Built the security foundation for jadepaw — a safety-configured wasmtime Engine
with Fuel+Epoch dual metering, 64MB memory hard caps via PoolingAllocationConfig,
a delegating chain ResourceLimiter, and the core type system (HostFunctions trait,
InstanceCapabilities, SessionState) that all subsequent phases depend on.

## What Was Built

### Task 1: Core types in jadepaw-core

- `SessionId`, `TenantId`, `ToolId` — UUID v7 newtype wrappers with Display, Deref, Serialize/Deserialize
- `JadepawError` enum — `CapabilityDenied`, `TrapError`, `PathValidationError` variants with Display + Error impls
- `HostFunctions` trait — canonical async trait with `log_message`, `file_read`, `file_write` methods; additive-only design
- `InstanceCapabilities` — 6-field struct with `can_read_files`, `can_write_files`, `can_exec_tools`, `can_network_to`, `max_memory_mb`, `max_compute_units`; `Default` = deny-all
- `PathPattern` and `DomainPattern` — newtype wrappers for whitelist pattern matching
- Tests: 9 passing (types.rs: 5, capabilities.rs: 4)

### Task 2: Engine factory, ResourceLimiter chain, and SessionState

- `EngineFactory::build()` — creates Engine with `consume_fuel(true)`, `epoch_interruption(true)`, `PoolingAllocationConfig(max_memory_size=64MB, max_unused_warm_slots=100)`, `InstanceAllocationStrategy::Pooling`
- `InstanceHardLimiter` — `ResourceLimiter` impl; returns `Err()` when desired > 64MB (Store poisoned, security boundary)
- `TenantQuotaLimiter` — wraps `InstanceHardLimiter`; tracks tenant aggregate budget via `Arc<AtomicUsize>`; returns `Ok(false)` on budget exceeded (recoverable); delegates to inner for hard cap
- `SessionState` — Store data struct with `session_id`, `tenant_id`, `capabilities`, `limits` (SessionLimits with InstanceHardLimiter), `created_at`
- `SessionLimits` — per-session limiter bundle
- `start_epoch_ticker()` — background thread at ~1ms intervals, exits on Engine drop (via EngineWeak) or EpochTickerGuard drop
- Tests: 11 passing (engine_smoke.rs: 3, limits.rs: 8) including integration tests for memory trap (guest grows >64MB) and fuel exhaustion (infinite loop)

## Deviations from Plan

### wasmtime 45.0 API Differences

The plan was written against the wasmtime API as documented in the research phase, but several
APIs differ in wasmtime 45.0. All were resolved as Rule 1 (bug fix) / Rule 3 (blocking) deviations:

**1. [Rule 3 - Blocking] `PoolingAllocatorConfig` renamed to `PoolingAllocationConfig`**
- **Found during:** Task 2
- **Issue:** The struct name was `PoolingAllocationConfig` in wasmtime 45.0, not `PoolingAllocatorConfig`
- **Fix:** Updated all references to use `PoolingAllocationConfig`
- **Files modified:** `crates/jadepaw-wasm/src/engine.rs`

**2. [Rule 3 - Blocking] `InstanceAllocationStrategy::Pooling` takes positional tuple arg**
- **Found during:** Task 2
- **Issue:** The variant is `Pooling(PoolingAllocationConfig)`, not `Pooling { config }`
- **Fix:** Changed to `InstanceAllocationStrategy::Pooling(pooling)`
- **Files modified:** `crates/jadepaw-wasm/src/engine.rs`

**3. [Rule 3 - Blocking] `ResourceLimiter` requires `table_growing` as a required method**
- **Found during:** Task 2
- **Issue:** wasmtime 45.0 `ResourceLimiter` trait requires both `memory_growing` AND `table_growing`
- **Fix:** Added `table_growing` impls to `InstanceHardLimiter` (always true) and `TenantQuotaLimiter` (delegates)
- **Files modified:** `crates/jadepaw-wasm/src/limits/instance_hard.rs`, `crates/jadepaw-wasm/src/limits/tenant_quota.rs`

**4. [Rule 3 - Blocking] `epoch_deadline_async_yield_and_update` returns `()` not `Result`**
- **Found during:** Task 2
- **Issue:** Method returns unit in wasmtime 45.0, `.expect()` chain doesn't compile
- **Fix:** Removed `.expect()` calls from all tests
- **Files modified:** `crates/jadepaw-wasm/tests/engine_smoke.rs`, `crates/jadepaw-wasm/tests/limits.rs`

**5. [Rule 3 - Blocking] `async_support` deprecated in wasmtime 45.0**
- **Found during:** Task 2
- **Issue:** `Config::async_support(true)` is deprecated with no effect in wasmtime 45.0
- **Fix:** Removed the call entirely (async support is always ON in wasmtime 45.0)
- **Files modified:** `crates/jadepaw-wasm/src/engine.rs`

**6. [Rule 3 - Blocking] `EngineWeak` does not have `increment_epoch()`**
- **Found during:** Task 2
- **Issue:** `increment_epoch()` is only on `Engine`, not on `EngineWeak`
- **Fix:** Clone the Engine into the background thread (Engine is Clone), use EngineWeak only for liveness check via `upgrade()`
- **Files modified:** `crates/jadepaw-wasm/src/epoch.rs`

**7. [Rule 1 - Bug] `wasmtime::Error::new()` requires `std::error::Error` implementor**
- **Found during:** Task 2
- **Issue:** `anyhow::Error` and `String` do not satisfy the trait bound for `wasmtime::Error::new()`
- **Fix:** Use `std::io::Error` (which implements `std::error::Error`) to wrap the error message
- **Files modified:** `crates/jadepaw-wasm/src/limits/instance_hard.rs`

**8. [Rule 1 - Bug] `uuid` crate missing `serde` feature**
- **Found during:** Task 1
- **Issue:** Workspace uuid dep had only `v7` feature, but types need `Serialize`/`Deserialize`
- **Fix:** Added `features = ["v7", "serde"]` in jadepaw-core's Cargo.toml
- **Files modified:** `crates/jadepaw-core/Cargo.toml`

## TDD Gate Compliance

Plan-level TDD gates verified:

1. **RED gate:** `cd93b29` — `test(02-01): add failing tests for core types` — types.rs + capabilities.rs tests compile-fail because types didn't exist
2. **GREEN gate:** `097ff78` — `feat(02-01): implement core types in jadepaw-core` — all 9 tests pass
3. **RED gate:** `552eb65` — `test(02-01): add failing tests for Engine, ResourceLimiter, and SessionState` — all 11 tests compile-fail because types didn't exist
4. **GREEN gate:** `a8edee5` — `feat(02-01): implement EngineFactory, ResourceLimiter chain, and SessionState` — all 11 tests pass

No REFACTOR commits were needed — the implementations were clean on first pass.

## Commits

| Hash | Type | Message |
|------|------|---------|
| cd93b29 | test | test(02-01): add failing tests for core types |
| 097ff78 | feat | feat(02-01): implement core types in jadepaw-core |
| 552eb65 | test | test(02-01): add failing tests for Engine, ResourceLimiter, and SessionState |
| a8edee5 | feat | feat(02-01): implement EngineFactory, ResourceLimiter chain, and SessionState |

## Verification

```bash
cargo test -p jadepaw-core && cargo test -p jadepaw-wasm
```

- jadepaw-core: 9 tests passed (5 types + 4 capabilities)
- jadepaw-wasm: 11 tests passed (3 engine_smoke + 8 limits)
- Total: 20 tests, 0 failures
- Build: clean (0 errors)

## Self-Check: PASSED

- [x] All source files exist on disk
- [x] All commits exist in git history
- [x] All tests pass
- [x] No build errors
- [x] No untracked files outside of `.planning/`