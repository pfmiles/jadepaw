# Phase 2: Wasm Isolation Core - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-30
**Phase:** 2-Wasm Isolation Core
**Areas discussed:** Guest-host interface, Instance pool lifecycle, ResourceLimiter & termination, Capability enforcement API

---

## Guest-Host Interface

| Option | Description | Selected |
|--------|-------------|----------|
| Trait-based contract + func_wrap | Rust trait in jadepaw-core defines canonical interface. jadepaw-wasm implements with func_wrap_async on core wasm modules. Full async, no WIT toolchain, migratable to WIT later. | ✓ |
| Full WIT Component Model | WIT files define the world. bindgen! generates host trait + guest stubs. Cross-language interop built in. | |
| Plain func_wrap (no abstraction) | Direct wasmtime Linker::func_wrap_async calls. Zero abstraction. Fastest to implement. | |

**User's choice:** Trait-based contract + func_wrap (Recommended)
**Notes:** WIT/component model rejected for Phase 2 due to async immaturity. Trait serves as versioned interface contract without buying into the full component model stack. Migration path to WIT remains open if cross-language guests become a requirement.

---

## Instance Pool Lifecycle

| Option | Description | Selected |
|--------|-------------|----------|
| Lazy instantiate + benchmark first | Pre-compile Module + InstancePre only. Acquire = Store::new + instantiate_async. Drop Store on release. Benchmark before optimizing. | ✓ |
| Pre-warmed Store pool from Day 1 | Pre-create N (Store, Instance) pairs at startup. Acquire = dequeue + inject session state. Release = guest _reset() + return to pool. | |
| Store sub-pool (partial pre-warm) | Pre-create Stores only. Acquire = dequeue Store + instantiate. Middle ground between lazy and full pre-warm. | |

**User's choice:** Lazy instantiate + benchmark first (Recommended)
**Notes:** Key constraint from wasmtime: Instance cannot outlive its Store, so pooling operates at Store granularity. Pooling allocator pre-allocates memory slots — lazy creation may already hit 5ms P99. Pre-warmed pool is an optimization to apply only if benchmarks prove it necessary.

---

## ResourceLimiter & Termination

| Option | Description | Selected |
|--------|-------------|----------|
| Custom monolithic ResourceLimiter | Single struct with per-instance caps + Arc<TenantQuota> for aggregate accounting. Ok(false) on tenant budget exceeded, Err() on 64MB hard cap. | ✓ |
| Built-in StoreLimits only | wasmtime StoreLimitsBuilder. No custom impl. Tenant quotas tracked externally. | |
| Delegating chain | TenantQuotaLimiter wraps InstanceHardLimiter. Composable, independently testable. | |

**User's choice:** Custom monolithic ResourceLimiter (Recommended)
**Notes:** Tiered semantics: tenant budget exceeded → Ok(false) (guest receives -1, recoverable); 64MB hard cap → Err() (trap, security boundary). Fuel + Epoch enabled at Engine level from Day 1, driven by background epoch-tick thread.

---

## Capability Enforcement API

| Option | Description | Selected |
|--------|-------------|----------|
| Check methods on SessionState | InstanceCapabilities struct in jadepaw-core. can_read_file, can_call_tool, can_access_domain methods on SessionState. Host functions call caller.data().can_*() at entry. | ✓ |
| bitflags gate + pattern list | Coarse O(1) bitflag gate at entry + PathPattern/DomainPattern list for fine-grained checks. Two-tier enforcement. | |
| Enforcement macro | check_capability! macro enforces uniformly. Centralized audit logging. Prevents "forgot to check" bugs. | |

**User's choice:** Check methods on SessionState (Recommended)
**Notes:** InstanceCapabilities in jadepaw-core (shared type). Enforcement methods in jadepaw-wasm on SessionState. Host functions call can_*() at the earliest possible point before side effects. Refactorable to macro or bitflag approach if host function count grows beyond ~10.

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

None — discussion stayed within phase scope.