# Phase 2: Wasm Isolation Core - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-30
**Phase:** 2-Wasm Isolation Core
**Areas discussed:** Guest-host interface, Instance pool lifecycle, ResourceLimiter & termination, Capability enforcement API

---

## Guest-Host Communication Interface

| Option | Description | Selected |
|--------|-------------|----------|
| Trait contract + func_wrap_async | Rust trait in jadepaw-core defines canonical interface. jadepaw-wasm implements with func_wrap_async on core wasm modules. Full async, no WIT toolchain, PoolingAllocator compatible, migratable to WIT later. | ✓ |
| WIT Component Model | WIT files define the world. bindgen! generates host trait + guest stubs. Cross-language interop. But Component Model is W3C Phase 1, async API immature. | |
| Plain func_wrap (no abstraction) | Direct wasmtime Linker::func_wrap_async calls. Zero abstraction. Fastest to implement but no type-level contract. | |
| Hybrid: Trait + WIT compatibility layer | Trait contract today + optional WIT shim later. Two interface definitions to maintain. | |

**User's choice:** Trait contract + func_wrap_async (Recommended)
**Notes:** Component Model rejected due to Phase 1 W3C status — async semantics still in active design, FutureReader requires manual lifecycle management, Rust async API nascent. Trait-based approach preserves full async support and PoolingAllocator compatibility. Migration path to WIT remains open when Component Model reaches Phase 3-4.

---

## Instance Pool Lifecycle

| Option | Description | Selected |
|--------|-------------|----------|
| Lazy instantiate + benchmark first | Pre-compile Module + InstancePre only. Acquire = Store::new + instantiate_async. Drop Store on release. Benchmark before optimizing. | ✓ |
| Pre-warmed (Store, Instance) hot pool | Pre-create N pairs at startup. Dequeue + inject session state. Requires guest reset contract. | |
| Store sub-pool | Pre-create Stores only. instantiate_async on acquire. Eliminates Store allocation but still pays instantiation cost. | |
| Two-tier pool | Hot pre-instantiated + cold InstancePre fallback. Best for sustained baseline + burst spikes. | |

**User's choice:** Lazy instantiate + benchmark first (Recommended)
**Notes:** PoolingAllocator pre-allocates memory slots at Engine creation, so Store::new is essentially slot assignment. InstancePre.instantiate_async benefits from warm slot affinity (max_unused_warm_slots=100). If profiling shows >3ms P99, Store Sub-pool is the natural first optimization before full hot pool.

---

## ResourceLimiter & Termination Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Delegating chain | InstanceHardLimiter (64MB → Err() trap) + TenantQuotaLimiter wraps it (aggregate budget → Ok(false)). Independently testable, extensible for future phases. | ✓ |
| Custom monolithic ResourceLimiter | Single struct with per-instance caps + Arc<TenantQuota>. Simple but couples security and business boundaries. | |
| StoreLimitsBuilder only | Built-in StoreLimitsBuilder for hard caps. Tenant quotas tracked externally via background polling. Zero custom code but eventual consistency. | |
| Layered with graceful recovery | Three-tier: allow → deny-soft (Ok(false)) → deny-hard (Err() after N denials). Guest gets recovery window but most complex design. | |
| External resource monitor | Separate tokio task polls metrics, signals termination via channel. Reactive not preventive, racy under burst allocation. | |

**User's choice:** Delegating chain (Recommended)
**Notes:** User explicitly chose this over monolithic after deep comparison. Key rationale: (1) InstanceHardLimiter can be audited in isolation for the security-critical "never exceed 64MB" invariant, (2) extensible — Phase 4 adds ToolRateLimiter, cluster mode swaps TenantQuotaLimiter for DistributedTenantQuotaLimiter without touching security boundary, (3) wasmtime's three-tier memory_growing semantics (Ok(true)/Ok(false)/Err()) naturally suggest layered design. Monolithic's simplicity advantage was deemed not worth the long-term coupling cost when security and business boundaries are fundamentally different concerns.

---

## Capability Enforcement API

| Option | Description | Selected |
|--------|-------------|----------|
| Check methods on SessionState | InstanceCapabilities struct in jadepaw-core. can_read_file, can_call_tool, can_access_domain methods on SessionState. Host functions call caller.data().can_*() at entry. | ✓ |
| Check methods + enforcement macro | check_capability! macro enforces uniformly. Centralized audit logging. Prevents "forgot to check" bugs. | |
| Two-tier: bitflags + pattern list | O(1) bitflag gate at entry + PathPattern/DomainPattern list for fine-grained checks. Two-tier enforcement. | |
| Typestate capability tokens | ZST tokens for compile-time enforcement. Architecturally elegant but fights wasmtime's func_wrap API. | |

**User's choice:** Check methods on SessionState (Recommended)
**Notes:** Right starting point for Phase 2's 5-10 host functions. Capability methods live in a dedicated `capability` module so migration to enforcement macro is a one-session refactor when Phase 4 host function count exceeds ~20. Integration test will verify every registered host function accesses caller.data() at entry. Typestate tokens rejected — runtime indirection needed to extract tokens from caller.data() defeats the compile-time guarantee.

---

## Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

## Deferred Ideas

None — discussion stayed within phase scope.