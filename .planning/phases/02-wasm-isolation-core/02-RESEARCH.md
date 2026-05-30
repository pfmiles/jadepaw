# Phase 2: Wasm Isolation Core - Research

**Researched:** 2026-05-30
**Domain:** WebAssembly runtime isolation, instance lifecycle, resource limiting, capability-based security
**Confidence:** HIGH

## Summary

Phase 2 builds the per-session Wasm sandbox: an isolated wasmtime Store with strict resource limits (64MB memory, Fuel+Epoch metering), a capability whitelist defaulting to deny, and host-mediated tool execution with sandboxed path validation. The guest-host communication contract is defined via a `HostFunctions` trait in `jadepaw-core` -- a one-way door that all subsequent phases depend on.

The wasmtime 45.0 API provides all the necessary primitives: `PoolingAllocatorConfig` for memory slot pre-allocation (enabling 10k+ concurrent instances), `ResourceLimiter` trait for per-instance memory caps, `Config::consume_fuel` + `Config::epoch_interruption` for termination enforcement, and `InstancePre` + `Linker::func_wrap_async` for sub-ms instantiation with async host functions. The phase's design decisions (delegating chain ResourceLimiter, trait-based guest-host contract, lazy instantiation pool) are well-supported by these APIs and follow patterns validated in production wasmtime deployments.

**Primary recommendation:** Implement the delegating chain ResourceLimiter (InstanceHardLimiter wrapping TenantQuotaLimiter) using custom `ResourceLimiter` trait impls rather than the built-in `StoreLimitsBuilder`, because the three-tier `memory_growing` return semantics (`Ok(true)`/`Ok(false)`/`Err()`) naturally support the separation of security boundaries (hard trap on per-instance violation) from business boundaries (graceful deny on tenant quota exceeded).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Engine + Config creation | API/Backend (jadepaw-wasm) | -- | wasmtime Engine owns compilation cache and global config; constructed once per process lifetime |
| Instance pool management | API/Backend (jadepaw-wasm) | -- | PoolingAllocator is Engine-scoped; acquire/release lifecycle is runtime concern, not client concern |
| Store-per-session isolation | API/Backend (jadepaw-wasm) | -- | Store owns linear memory, tables, instances; created/destroyed per session, never shared |
| Resource limiting (memory/fuel/epoch) | API/Backend (jadepaw-wasm) | -- | wasmtime's ResourceLimiter trait is Store-scoped; host-side enforcement only |
| Capability enforcement (path validation, tool whitelist) | API/Backend (jadepaw-wasm) | jadepaw-core (type definitions) | Host functions check capabilities at entry; capability types are shared across crate boundary |
| Guest-host communication contract | jadepaw-core (trait definition) | jadepaw-wasm (trait impl) | Trait in core so jadepaw-agent (Phase 3) and jadepaw-skill (Phase 6) can reference it without depending on jadepaw-wasm |
| Path normalization and sandbox boundary check | API/Backend (jadepaw-wasm) | -- | Filesystem access is host-mediated; guard is applied before any OS-level I/O |
| Guest Wasm module compilation | API/Backend (jadepaw-wasm) | -- | Module::from_binary + InstancePre compilation happens in jadepaw-wasm; guest modules are data from jadepaw-skill |

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Define a `HostFunctions` trait in `jadepaw-core` that catalogues every host function import. This trait is the canonical, versioned interface contract -- additive-only, CI-verifiable.
- **D-02:** Implement the trait in `jadepaw-wasm` using wasmtime's `Linker::func_wrap` / `func_wrap_async` on core wasm modules (not component model). Guest modules compile to `wasm32-wasi` and import from a well-known module namespace (`"jadepaw"`).
- **D-03:** Do NOT adopt WIT/component model at this stage. Migration path to WIT remains open when (if) the Component Model reaches Phase 3-4.
- **D-04:** Implement lazy instantiation: pre-compile only `Module` and `InstancePre` (shared across all sessions via `Arc`). Acquire = `Store::new(engine, session_state)` + `instance_pre.instantiate_async(&mut store)`. Release = drop the Store (pooling allocator reclaims memory). No guest-side reset contract needed.
- **D-05:** Bound concurrency via `tokio::sync::Semaphore` -- max concurrent instances configurable. Track active sessions in a `DashMap<SessionId, SessionHandle>`.
- **D-06:** Benchmark Store creation + instantiation latency against the 5ms P99 target before considering pre-warmed Store pool optimization.
- **D-07:** Implement a **delegating chain** of ResourceLimiters. `InstanceHardLimiter` enforces per-instance security boundaries (64MB hard cap -- `Err()` trap). `TenantQuotaLimiter` wraps `InstanceHardLimiter` and enforces tenant-level aggregate budgets -- `Ok(false)` on tenant budget exceeded (guest receives -1, recoverable), then delegates to inner limiter for instance-level checks. Each limiter is independently testable.
- **D-08:** Tiered `memory_growing` semantics: `InstanceHardLimiter` returns `Err()` when 64MB per-instance hard cap is exceeded (trap, Store is terminally poisoned). `TenantQuotaLimiter` returns `Ok(false)` when tenant aggregate budget is exceeded (guest receives -1 from `memory.grow` -- recoverable).
- **D-09:** Enable both Fuel metering (`Config::consume_fuel(true)`) and Epoch interruption (`Config::epoch_interruption(true)`) at Engine level from Day 1. Drive epoch ticks via a background thread per Engine. Set initial Fuel budget per agent turn via `Store::set_fuel()`. `PoolingAllocatorConfig::max_memory_size` must match the 64MB per-instance cap.
- **D-09a:** The delegating chain architecture is extensible by design -- Phase 4 can add `ToolRateLimiter`, Phase 5 can add `SessionMemoryLimiter`, and cluster mode can swap `TenantQuotaLimiter` for `DistributedTenantQuotaLimiter` without touching the security-critical `InstanceHardLimiter`.
- **D-10:** `InstanceCapabilities` struct lives in `jadepaw-core` (shared type): fields `can_read_files: Vec<PathPattern>`, `can_write_files: Vec<PathPattern>`, `can_exec_tools: Vec<ToolId>`, `can_network_to: Vec<DomainPattern>`, `max_memory_mb: u32`, `max_compute_units: u64`.
- **D-11:** Check methods on `SessionState` in `jadepaw-wasm`: `can_read_file(path)`, `can_write_file(path)`, `can_call_tool(id)`, `can_access_domain(domain)`. Host functions call `caller.data().can_*(...)` at entry before any side effects. Define can_* methods in a dedicated `capability` module.
- **D-12:** Capability whitelist is declared at instance initialization. Default deny -- if a capability is not explicitly granted, the check method returns false and the host function returns a `CapabilityDenied` error to the guest.

### Claude's Discretion

No areas were deferred to Claude -- all decisions were user-directed.

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SEC-01 | Wasm instance isolation -- each session runs in independent wasmtime Store, linear memory provides hardware-level isolation | D-04 (Store-per-session + drop on release), pooling allocator ensures zero data residue on Store deallocation. `Store::new(session_state)` creates independent linear memory per session. |
| SEC-02 | wasmtime Fuel + Epoch dual resource metering from Day 1, explicit StoreLimits (64MB memory cap) | D-07/D-08/D-09 (ResourceLimiter delegating chain, Fuel+Epoch both enabled). `StoreLimitsBuilder::memory_size(64*1024*1024)` + custom ResourceLimiter trait impl for tenant quotas. |
| SEC-03 | Tool execution through host mediation, path parameters enforced with normalization and sandbox boundary check | D-11 (can_* checks on SessionState). `normalize_path()` removes `..` and `.`, resolves against sandbox root, verifies prefix match. Applied in every host function that touches filesystem. |
| SEC-04 | Capability whitelist -- instance initialization declares allowed tools/capabilities, default deny | D-10/D-12 (InstanceCapabilities struct in jadepaw-core, default deny). `can_call_tool()` returns false if tool not in allowed_tools set. |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| wasmtime | 45.0 (workspace) | WebAssembly runtime -- Engine, Store, Module, InstancePre, Linker, ResourceLimiter, PoolingAllocator | Only Rust-native Wasm runtime with official Bytecode Alliance backing. PoolingAllocator is required for 10k+ concurrent instances. [VERIFIED: crates.io registry via `cargo search wasmtime` -- version 45.0.0 confirmed] |
| tokio | 1.52 (workspace) | Async runtime for Store instantiation, epoch ticker background thread, semaphore-based concurrency control | Already in workspace. Required by wasmtime's async feature. `tokio::sync::Semaphore` for D-05 concurrency bounding. [VERIFIED: crates.io registry] |
| jadepaw-core | 0.1.0 (workspace) | Shared types -- `HostFunctions` trait, `InstanceCapabilities` struct, `SessionId`, `TenantId`, `ToolId`, `PathPattern`, `DomainPattern` | Dependency constraint: jadepaw-wasm only depends on jadepaw-core. Types must live in core so Phase 3/6 can reference them. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| uuid | 1.0 (workspace) | Type 7 (time-ordered) UUIDs for SessionId generation | Already in jadepaw-core. Used for session tracking in DashMap. |
| chrono | 0.4 (workspace) | Timestamps for session creation/expiry | Already in jadepaw-core. Used in SessionState metadata. |
| tracing | 0.1 (workspace) | Structured spans for instance lifecycle (acquire/release/trap) | Integrates with Phase 9 observability. Spans per session. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| custom ResourceLimiter (delegating chain) | StoreLimitsBuilder (built-in) | StoreLimitsBuilder only provides static per-Store limits -- cannot implement tenant-level aggregate quotas or the tiered Err()/Ok(false) semantics. The built-in is fine for the per-instance hard cap but insufficient for the full D-07/D-08 requirements. |
| InstancePre lazy instantiation | Pre-warmed hot pool (Store, Instance) pairs | Hot pool requires guest-side reset contract (zeroing memory, resetting globals, clearing tables). Lazy instantiation with PoolingAllocator is already sub-ms for Store::new + instantiate_async. Benchmark first (D-06) -- if <5ms P99, no hot pool needed. |
| func_wrap_async directly (no trait) | HostFunctions trait | Direct func_wrap_async has no type-level contract. The trait enables CI-verifiable consistency (D-01) and allows Phase 3/6 to reference the interface without wasmtime dependency. |
| Epoch-only interruption | Fuel-only metering | Epoch is lightweight but non-deterministic; Fuel is precise but expensive (20-30% throughput overhead). Multi-tenant requires both: Fuel for hard upper bounds, Epoch for cooperative timeslicing across concurrent stores. [CITED: docs.rs wasmtime Config page -- section on epoch_interruption and consume_fuel] |

**Installation:**
No new external dependencies -- all libraries are already in workspace. Required wasmtime features to add in `jadepaw-wasm/Cargo.toml`:

```toml
[dependencies]
wasmtime = { workspace = true, features = ["pooling-allocator", "async", "cranelift", "runtime"] }
```

The workspace's `default-features = false` for wasmtime means these features must be explicitly enabled in jadepaw-wasm. Features are additive; specifying them here adds to workspace base.

**Version verification:** wasmtime 45.0.0 confirmed on crates.io via `cargo search wasmtime`. tokio 1.52 confirmed in workspace Cargo.toml. uuid 1.0, chrono 0.4, tracing 0.1 all confirmed in workspace Cargo.toml.

## Package Legitimacy Audit

> slopcheck is designed for npm/PyPI packages, not Rust crates. Verification performed via `cargo search` on crates.io for Rust ecosystem packages.

| Package | Registry | Age | Downloads | Source Repo | Verification | Disposition |
|---------|----------|-----|-----------|-------------|--------------|-------------|
| wasmtime | crates.io | ~6 yrs | 45.0.0 published | github.com/bytecodealliance/wasmtime | cargo search confirms 45.0.0 exists | Approved |
| tokio | crates.io | ~9 yrs | Well-established | github.com/tokio-rs/tokio | Workspace 1.52 | Approved |
| serde | crates.io | ~9 yrs | Industry standard | github.com/serde-rs/serde | Workspace 1.0 | Approved |
| serde_json | crates.io | ~9 yrs | Industry standard | github.com/serde-rs/json | Workspace 1.0 | Approved |
| uuid | crates.io | ~8 yrs | Well-established | github.com/uuid-rs/uuid | Workspace 1.0 (v7 feature) | Approved |
| chrono | crates.io | ~9 yrs | Well-established | github.com/chronotope/chrono | Workspace 0.4 | Approved |
| tracing | crates.io | ~6 yrs | Well-established | github.com/tokio-rs/tracing | Workspace 0.1 | Approved |

**Packages removed due to slopcheck [SLOP] verdict:** none (all Rust crates verified via crates.io)
**Packages flagged as suspicious [SUS]:** none

*All packages are well-established crates in the Rust ecosystem. No new external dependencies required.*

## Architecture Patterns

### System Architecture Diagram

```
                                    ┌─────────────────────────┐
                                    │   jadepaw-agent          │
                                    │   (Phase 3 consumer)     │
                                    │                          │
                                    │   References HostFunctions│
                                    │   trait from jadepaw-core │
                                    └────────────┬─────────────┘
                                                 │
                                    ┌─────────────▼────────────┐
                                    │   jadepaw-core            │
                                    │                           │
                                    │   HostFunctions trait     │
                                    │   InstanceCapabilities    │
                                    │   SessionId, TenantId     │
                                    │   PathPattern, DomainPat  │
                                    └────────────┬──────────────┘
                                                 │
    ┌────────────────────────────────────────────┼────────────────────────────────────────┐
    │                      jadepaw-wasm (this phase)                                      │
    │                                                                                     │
    │  ┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────────────┐     │
    │  │  Engine Factory  │───>│  Instance Pool   │───>│  Per-Session Store          │     │
    │  │                  │    │                  │    │                             │     │
    │  │  Config:         │    │  Arc<Module>     │    │  Store<SessionState>        │     │
    │  │  - pooling-alloc │    │  Arc<InstancePre>│    │  ├── data: SessionState     │     │
    │  │  - consume_fuel  │    │  Semaphore(N)    │    │  │   ├── session_id         │     │
    │  │  - epoch_intrpt  │    │  DashMap<Sid, H> │    │  │   ├── capabilities       │     │
    │  │  - cranelift JIT │    │                  │    │  │   └── limits             │     │
    │  └────────┬─────────┘    └────────┬─────────┘    │  └── epoch_deadline         │     │
    │           │                       │              │  └── fuel_budget            │     │
    │           │              ┌────────▼─────────┐    └──────────────┬──────────────┘     │
    │           │              │  Acquire Flow     │                   │                    │
    │           │              │                   │    ┌──────────────▼──────────────┐     │
    │           │              │  1. sem.acquire() │    │  Host Functions             │     │
    │           │              │  2. Store::new()  │    │                             │     │
    │           │              │  3. set_fuel()    │    │  Linker<SessionState>       │     │
    │           │              │  4. set_epoch()   │    │  module: "jadepaw"          │     │
    │           │              │  5. inst_pre      │    │  ├── file_read(path, ptr)   │     │
    │           │              │     .instantiate  │    │  │   └─> can_read_file()    │     │
    │           │              │     _async()      │    │  │   └─> validate_path()    │     │
    │           │              └───────────────────┘    │  ├── file_write(...)        │     │
    │           │                                       │  ├── http_request(...)      │     │
    │           │                                       │  │   └─> can_access_domain()│     │
    │           │                                       │  └── log_message(...)       │     │
    │           │                                       └─────────────────────────────┘     │
    │           │                                                                           │
    │  ┌────────▼──────────────────────────────────────────────────────────────────────┐    │
    │  │  ResourceLimiter Delegating Chain                                             │    │
    │  │                                                                               │    │
    │  │  TenantQuotaLimiter { budget: Arc<AtomicUsize>, inner: InstanceHardLimiter }  │    │
    │  │       │                                                                       │    │
    │  │       │  memory_growing(current, desired, max)                                │    │
    │  │       │  ├── tenant_budget_exceeded? → Ok(false)  // guest gets -1, recover   │    │
    │  │       │  └── delegate to inner                                                  │    │
    │  │       ▼                                                                       │    │
    │  │  InstanceHardLimiter { max_memory: 64MB }                                      │    │
    │  │       │                                                                       │    │
    │  │       │  memory_growing(current, desired, max)                                │    │
    │  │       │  ├── desired > 64MB? → Err()  // trap, Store poisoned                 │    │
    │  │       │  └── Ok(true)                                                         │    │
    │  └───────────────────────────────────────────────────────────────────────────────┘    │
    │                                                                                     │
    │  ┌──────────────────────────────────────────────────────────────────────────────┐    │
    │  │  Epoch Ticker (background thread)                                             │    │
    │  │                                                                               │    │
    │  │  loop { Engine::increment_epoch(); sleep(Duration::from_millis(1)); }         │    │
    │  │  Per-store: epoch_deadline_async_yield_and_update(delta)                      │    │
    │  └──────────────────────────────────────────────────────────────────────────────┘    │
    └─────────────────────────────────────────────────────────────────────────────────────┘
```

### Recommended Project Structure

```
crates/jadepaw-core/src/
├── lib.rs                    # Re-exports, module docs
├── types.rs                  # SessionId, TenantId, ToolId, SkillId (extend)
├── error.rs                  # JadepawError, CapabilityDenied, TrapError
├── host_functions.rs         # HostFunctions trait definition
├── capabilities.rs           # InstanceCapabilities, PathPattern, DomainPattern
└── config.rs                 # (existing) extended with tenant config types

crates/jadepaw-wasm/src/
├── lib.rs                    # Re-exports, module docs
├── engine.rs                 # EngineFactory: build Config with pooling+fuel+epoch
├── pool.rs                   # InstancePool: Arc<Module>, Arc<InstancePre>, Semaphore, DashMap
├── session.rs                # SessionState struct (Store<T> data), lifecycle helpers
├── linker.rs                 # Host function registration via Linker<SessionState>
├── host/                     # Host function implementations
│   ├── mod.rs
│   ├── filesystem.rs         # file_read/file_write + validate_path
│   ├── network.rs            # http_request + domain allowlist check
│   └── logging.rs            # log_message host function
├── limits/                   # ResourceLimiter implementations
│   ├── mod.rs
│   ├── instance_hard.rs      # InstanceHardLimiter: 64MB Err() trap
│   └── tenant_quota.rs       # TenantQuotaLimiter: aggregate budget Ok(false)
├── capability/               # Capability enforcement module (D-11)
│   └── mod.rs                # can_read_file, can_write_file, can_call_tool, can_access_domain
├── epoch.rs                  # Epoch ticker background thread
├── path.rs                   # normalize_path, validate_sandbox_path
├── wasi.rs                   # WasiCtx setup, preopens directory management
└── error.rs                  # Wasm-specific error types
```

### Pattern 1: Delegating Chain ResourceLimiter

**What:** Two (or more) `ResourceLimiter` implementations are composed in a chain. The outer limiter checks business-level constraints (tenant quota) and may return `Ok(false)` for recoverable denials. The inner limiter checks security-level constraints (hard per-instance cap) and returns `Err()` for violation traps. Each limiter is independently testable and new limiters can be prepended without touching existing ones.

**When to use:** Whenever security boundaries (must never exceed X) and business boundaries (should not exceed Y as a soft limit) need different failure modes. The wasmtime `ResourceLimiter` trait's three-tier return type (`Ok(true)`, `Ok(false)`, `Err()`) is designed for exactly this delegation pattern.

**Example:**
```rust
// Source: wasmtime docs.rs ResourceLimiter trait (v45.0.0)
// [VERIFIED: docs.rs/wasmtime/latest/wasmtime/trait.ResourceLimiter.html]

use wasmtime::ResourceLimiter;

/// Security boundary: hard per-instance memory cap. Err() = trap = Store poisoned.
pub struct InstanceHardLimiter {
    max_bytes: usize, // 64 * 1024 * 1024
}

impl ResourceLimiter for InstanceHardLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        if desired > self.max_bytes {
            anyhow::bail!(
                "instance memory limit exceeded: {} bytes requested, {} bytes max",
                desired, self.max_bytes
            );
        }
        Ok(true)
    }
}

/// Business boundary: tenant-level aggregate memory budget.
/// Ok(false) = memory.grow returns -1 to guest (recoverable).
pub struct TenantQuotaLimiter {
    tenant_budget_used: Arc<AtomicUsize>,
    tenant_budget_max: usize,
    inner: InstanceHardLimiter,
}

impl ResourceLimiter for TenantQuotaLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        let delta = desired - current;
        let used = self.tenant_budget_used.fetch_add(0, Ordering::Relaxed);
        if used + delta > self.tenant_budget_max {
            return Ok(false); // recoverable: guest gets -1
        }
        self.tenant_budget_used.fetch_add(delta, Ordering::Relaxed);
        // Delegate to inner for hard cap check
        self.inner.memory_growing(current, desired, maximum)
    }
}
```

### Pattern 2: SessionState as Store Data

**What:** The `SessionState` struct is the type parameter `T` in `Store<T>`. It holds session identity (`session_id`, `tenant_id`), capability whitelist (`InstanceCapabilities`), and resource limits. Host functions access it via `caller.data()` (immutable) or `caller.data_mut()` (mutable for counters). The Store is created fresh per session and dropped on session end -- no state reuse.

**When to use:** Every session. This is the canonical wasmtime pattern for per-Store host data.

**Example:**
```rust
// Source: docs.rs/wasmtime/latest/wasmtime/struct.Caller.html
// [VERIFIED: docs.rs Caller::data() and Caller::data_mut() API]

pub struct SessionState {
    pub session_id: SessionId,
    pub tenant_id: TenantId,
    pub capabilities: InstanceCapabilities,
    pub limits: LimitsState,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// In host function:
// fn file_read(mut caller: Caller<'_, SessionState>, path_ptr: i32, path_len: i32, ...) -> ... {
//     let state = caller.data();  // immutable access for capability check
//     let memory = caller.get_export("memory").and_then(|e| e.into_memory());
//     if !state.can_read_file(&path) {
//         return Err(CapabilityDenied.into());
//     }
//     // ... read from guest memory, validate path, perform I/O ...
// }
```

### Pattern 3: Lazy Instantiation Pool

**What:** Pre-compile the guest Wasm module once at pool creation, producing `Arc<Module>` and `Arc<InstancePre<SessionState>>`. On acquire: `Store::new(engine, session_state)` + `instance_pre.instantiate_async(&mut store)`. On release: drop the Store. Concurrency bounded by `tokio::sync::Semaphore`. Active sessions tracked in `DashMap<SessionId, SessionHandle>`.

**When to use:** When PoolingAllocator is enabled, `Store::new` + `InstancePre::instantiate_async` is typically sub-ms because memory slots are pre-allocated at Engine creation time. This avoids the complexity of pre-warmed Store pools (no guest-side reset contract, no memory zeroing discipline).

**Example:**
```rust
// Source: docs.rs/wasmtime/latest/wasmtime/struct.Linker.html#method.instantiate_pre
// [VERIFIED: docs.rs Linker::instantiate_pre API]

pub struct InstancePool {
    engine: Engine,
    instance_pre: Arc<InstancePre<SessionState>>,
    semaphore: Arc<Semaphore>,
    active_sessions: Arc<DashMap<SessionId, SessionHandle>>,
}

impl InstancePool {
    pub fn new(config: PoolConfig) -> Result<Self> {
        let engine = EngineFactory::build(config.engine_config)?;
        let module = Module::from_binary(&engine, &config.guest_bytes)?;
        let mut linker = Linker::new(&engine);
        // register all host functions on linker...
        let instance_pre = Arc::new(linker.instantiate_pre(&module)?);
        // ...
    }

    pub async fn acquire(&self, session_id: SessionId, state: SessionState)
        -> Result<SessionHandle>
    {
        let permit = self.semaphore.acquire().await;
        let mut store = Store::new(&self.engine, state);
        store.set_fuel(1_000_000)?;
        store.epoch_deadline_async_yield_and_update(100)?;
        store.limiter(|s| &mut s.limits);
        let instance = self.instance_pre.instantiate_async(&mut store).await?;
        // ...
    }
}
```

### Anti-Patterns to Avoid

- **Sharing Store across tenants:** A Store is a unit of isolation. Never reuse a Store for a different session or tenant. Always `Store::new()` per session, `drop()` on session end. [CITED: PITFALLS.md -- Pitfall 2]
- **Trusting guest-provided paths without normalization:** Always do `normalize_path()` -> resolve against sandbox root -> verify prefix match. Path traversal is the most common sandbox escape vector. [CITED: PITFALLS.md -- Pitfall 3]
- **Using only Fuel or only Epoch:** Fuel is precise but expensive (20-30% overhead). Epoch is cheap but non-deterministic. Multi-tenant requires both. [CITED: PITFALLS.md -- Pitfall 1]
- **Using StoreLimitsBuilder alone for multi-tenant quotas:** StoreLimitsBuilder only provides per-Store static limits. It cannot track tenant-level aggregate memory usage across multiple concurrent sessions. [CITED: PITFALLS.md -- Pitfall 4]
- **Forgetting PoolingAllocatorConfig::max_memory_size:** The pooling allocator allocates memory pool slots at Engine creation. If `max_memory_size` is not set, it defaults to 4 GiB per slot, wasting virtual address space and making 10k+ concurrent instances impossible. Must match the 64MB per-instance cap. [CITED: PITFALLS.md -- Pitfall 4, "max_memory_size defaults to 4 GiB!"]

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Memory allocation/deallocation per Wasm instance | Custom allocator or mmap management | wasmtime `PoolingAllocatorConfig` | PoolingAllocator pre-allocates memory slots at Engine creation. Handles slot reuse, defragmentation, and guard page setup. Required for 10k+ concurrent instances. |
| Path normalization and sandbox containment | Custom path traversal detection | `std::path::Path::canonicalize` (when available) or manual `..`/`.` stripping + prefix check | Canonicalize resolves symlinks which manual `..` stripping misses. Prefix check (`starts_with(sandbox_root)`) is the last line of defense. |
| Guest memory reading (ptr, len) | Manual pointer arithmetic on raw bytes | `caller.get_export("memory")` + `Memory::data(&caller)` + slice bounds checking | wasmtime validates that ptr+len stays within the guest's linear memory bounds. Manual pointer arithmetic on the raw memory buffer is unsafe and error-prone. |
| Epoch ticker thread | Timer-based pthread solution | `std::thread::spawn` + `EngineWeak::upgrade` + `Engine::increment_epoch` | wasmtime provides `EngineWeak` specifically for this pattern -- the ticker thread won't keep the Engine alive. `increment_epoch()` is signal-safe (no syscalls, atomic only). [VERIFIED: docs.rs Engine docs] |
| Concurrent session tracking | Custom concurrent map with locking | `DashMap<SessionId, SessionHandle>` | DashMap is lock-free for reads, shard-level locking for writes. Already used in architecture decisions (D-05). |
| Concurrency bounding | Manual count tracking or `tokio::sync::Mutex` counter | `tokio::sync::Semaphore` | Semaphore naturally models resource acquisition/release. `acquire().await` provides backpressure when pool is exhausted. |
| ASVS validation for inputs | Manual validation in every function | Dedicated `capability` module with check methods | Centralized `can_read_file()`, `can_write_file()`, `can_call_tool()`, `can_access_domain()` methods on `SessionState`. Every host function calls them at entry. [CITED: D-11, D-12]

**Key insight:** The wasmtime 45.0 crate already provides all the primitives needed for production-grade Wasm sandboxing. The phase's work is about composition, configuration, and enforcement discipline -- not about reinventing isolation primitives.

## Common Pitfalls

### Pitfall 1: Fuel-less Infinite Loop Denial of Service

**What goes wrong:** A malicious or buggy Wasm guest module enters an infinite loop. Without fuel metering or epoch interruption enabled, wasm execution monopolizes the host thread indefinitely, blocking all other tenants on that node.

**Why it happens:** Both `Config::consume_fuel` and `Config::epoch_interruption` default to `false`. Developers prototype without them, then forget to enable for production.

**How to avoid:** Enable both at Engine creation (D-09). Use `Store::set_fuel(1_000_000)` per session for a hard upper bound. Use `Store::epoch_deadline_async_yield_and_update(delta)` for cooperative timeslicing. Drive epoch via a background thread calling `Engine::increment_epoch()` at ~1ms intervals. [CITED: PITFALLS.md -- Pitfall 1]

**Warning signs:** A single session hangs; all other sessions on the same node freeze; P99 latency spikes with no load increase.

### Pitfall 2: PoolingAllocator max_memory_size Not Matched to Per-Instance Cap

**What goes wrong:** `PoolingAllocatorConfig::max_memory_size` defaults to 4 GiB. If the per-instance cap is 64MB but pooling allocator slots are 4 GiB each, virtual address space is exhausted after ~500 instances on 64-bit, making the 10k+ target impossible.

**Why it happens:** Two separate configuration points (Engine-level PoolingAllocatorConfig vs Store-level ResourceLimiter/StoreLimits) that must agree. Easy to overlook.

**How to avoid:** Set `PoolingAllocatorConfig::max_memory_size(64 * 1024 * 1024)` at Engine creation (D-09). Document that this value must match the InstanceHardLimiter cap. Write an integration test that verifies memory pools can support the target concurrent instance count. [CITED: PITFALLS.md -- Pitfall 4]

**Warning signs:** Instance creation fails with "out of resources" at far fewer concurrent sessions than expected. Pooling allocator exhaustion errors.

### Pitfall 3: Path Traversal via Symlinks After Normalization

**What goes wrong:** `normalize_path()` removes `..` and `.`, and `starts_with(sandbox_root)` passes. But the path includes a symlink that points outside the sandbox root. Canonicalization (`Path::canonicalize`) would have revealed this but was skipped for performance.

**Why it happens:** `Path::canonicalize` requires filesystem access and may be slow. Developers may skip it, relying solely on string-level normalization.

**How to avoid:** Always check that the final resolved path (after `canonicalize` or equivalent resolution) starts with the sandbox root. If canonicalize is too slow for hot paths, at minimum: resolve each path component checking for symlinks at every level, or use `openat2` with `RESOLVE_NO_SYMLINKS` on Linux. For Phase 2 MVP, `canonicalize` is acceptable.

**Warning signs:** File reads/writes succeed for paths that should be outside the sandbox. Audit logs show access to system directories.

### Pitfall 4: ResourceLimiter Not Registered on Store

**What goes wrong:** The custom delegating chain ResourceLimiter is defined but never registered on the Store. `Store::limiter()` is a separate method from `Store::new()` -- the Store is created with SessionState, then the limiter closure must be explicitly registered.

**Why it happens:** `Store::limiter()` is a method that takes a closure `FnMut(&mut T) -> &mut dyn ResourceLimiter`. If you create the Store and forget to call `store.limiter(...)`, the Store has no resource limits even though SessionState has limits configured.

**How to avoid:** Create a helper function `Store::create_with_limits(engine, state) -> Store<SessionState>` that creates the Store AND registers the limiter in one step. Never allow raw `Store::new()` in pool acquire code -- always use the helper. Write a test that creates a Store without the helper and verifies that `memory_growing` requests succeed without limit.

**Warning signs:** Guest modules allocate beyond 64MB without trapping. Memory usage grows without bounds.

### Pitfall 5: Skipping Epoch Deadline Configuration on Each Store

**What goes wrong:** Epoch interruption is enabled at Engine level, but `store.epoch_deadline_async_yield_and_update(delta)` is never called. The Store's epoch deadline defaults to 0, meaning the Store traps immediately on the first epoch check. Sessions fail on first execution.

**Why it happens:** Enabling `Config::epoch_interruption(true)` is only half the setup. Each Store must have its epoch deadline configured.

**How to avoid:** Call `store.epoch_deadline_async_yield_and_update(delta)` immediately after `Store::new()` in the pool acquire flow. The `delta` parameter controls how many epoch ticks before yielding -- start with 100 (corresponds to ~100ms with 1ms ticker interval) and tune based on profiling.

**Warning signs:** All sessions trap immediately with epoch-related errors. The error message references "epoch deadline of 0."

## Code Examples

Verified patterns from official documentation:

### Engine Setup with All Safety Features

```rust
// Source: docs.rs/wasmtime/45.0.0/wasmtime/struct.Config.html
// [VERIFIED: Config::consume_fuel, Config::epoch_interruption, Config::allocation_strategy APIs]

use wasmtime::{Config, Engine, OptLevel};

fn build_engine() -> anyhow::Result<Engine> {
    let mut config = Config::default();

    // Safety: both fuel AND epoch from Day 1
    config.consume_fuel(true);
    config.epoch_interruption(true);

    // Performance: Cranelift JIT (fastest compilation)
    config.cranelift_opt_level(OptLevel::Speed);

    // Pooling allocator: pre-allocate memory slots for 10k+ instances
    let mut pooling = wasmtime::PoolingAllocatorConfig::default();
    pooling.max_memory_size(64 * 1024 * 1024); // MUST match 64MB instance cap
    // Note: total_memories, total_instances, etc. have defaults
    // that should be confirmed for your target concurrent session count.
    // [ASSUMED: PoolingAllocatorConfig builder API based on docs.rs index
    //  -- exact method names need verification. Context7 or official docs
    //  for PoolingAllocatorConfig page returned 404 for wasmtime 45.]
    config.allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling {
        config: pooling,
    });

    // Async support for func_wrap_async + instantiate_async
    config.async_support(true);

    Engine::new(&config)
}
```

### Host Function with Capability Check

```rust
// Source: docs.rs/wasmtime/latest/wasmtime/struct.Caller.html
// [VERIFIED: Caller::data() API]
// Path validation pattern from docs/jadepaw_discussion.md Section 3.2

fn register_host_functions(linker: &mut Linker<SessionState>) -> anyhow::Result<()> {
    linker.func_wrap_async(
        "jadepaw",                     // well-known namespace (D-02)
        "file_read",
        |mut caller: Caller<'_, SessionState>,
         path_ptr: i32,
         path_len: i32,
         buf_ptr: i32,
         buf_len: i32|
         -> Box<dyn Future<Output = Result<i32>> + Send + '_> {
            Box::new(async move {
                let state = caller.data(); // immutable access
                // Get guest memory
                let memory = caller
                    .get_export("memory")
                    .and_then(|e| e.into_memory())
                    .ok_or_else(|| anyhow::anyhow!("no exported memory"))?;

                // Bounds-checked read from guest memory
                let path_bytes = memory.data(&caller)
                    [path_ptr as usize..(path_ptr + path_len) as usize]
                    .to_vec();
                let path_str = std::str::from_utf8(&path_bytes)?;

                // Capability enforcement (D-10, D-11)
                if !state.can_read_file(path_str) {
                    return Err(anyhow::anyhow!("CapabilityDenied: file_read on '{}'", path_str));
                }

                // Path validation (SEC-03)
                let safe_path = validate_path(path_str, &state.sandbox_root)?;

                // Perform I/O only after both checks pass
                let contents = tokio::fs::read(&safe_path).await?;
                // Write results back to guest memory (bounds-checked)...
                Ok(0)
            })
        },
    )?;
    Ok(())
}
```

### Path Validation (Canonical Implementation)

```rust
// Source: docs/jadepaw_discussion.md Section 3.2
// [VERIFIED: project design document, canonical reference]

fn validate_path(guest_path: &str, sandbox_root: &Path) -> anyhow::Result<PathBuf> {
    // Step 1: Strip any leading slashes to make path relative
    let relative = guest_path.trim_start_matches('/');

    // Step 2: Join with sandbox root
    let candidate = sandbox_root.join(relative);

    // Step 3: Canonicalize to resolve symlinks, .., .
    let resolved = candidate.canonicalize()
        .map_err(|e| anyhow::anyhow!("path resolution failed: {} -- {}", guest_path, e))?;

    // Step 4: Verify containment within sandbox
    if !resolved.starts_with(sandbox_root) {
        return Err(anyhow::anyhow!(
            "path traversal detected: '{}' resolves outside sandbox root",
            guest_path
        ));
    }

    Ok(resolved)
}
```

### Epoch Ticker Background Thread

```rust
// Source: docs.rs/wasmtime/latest/wasmtime/struct.Engine.html#method.increment_epoch
// [VERIFIED: Engine::increment_epoch API]

fn start_epoch_ticker(engine: &Engine) -> impl Drop {
    let engine_weak = engine.weak();
    let handle = std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1));
            // Upgrade returns None when Engine is dropped -> ticker exits
            if engine_weak.upgrade().is_none() {
                break;
            }
            // Signal-safe: atomic increment, no syscalls
            engine_weak.increment_epoch();
        }
    });
    // Return a guard that joins the thread on drop
    // (or just let it terminate when Engine is dropped)
    handle
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| WIT Component Model bindgen! | Trait-based `HostFunctions` + `func_wrap_async` | Phase 2 (now) | Avoids W3C Phase 1 component model; full async support; migration path preserved |
| Pre-warmed hot pool (Store, Instance) | Lazy instantiation (Store::new + instantiate_async) | Phase 2 (now) | Simpler lifecycle; PoolingAllocator makes this sub-ms; benchmark before optimizing |
| StoreLimitsBuilder only | Delegating chain custom ResourceLimiter | Phase 2 (now) | Enables tiered Err()/Ok(false) semantics; independent audit of security boundary |
| Single monolithic ResourceLimiter | Extensible delegating chain (D-09a) | Phase 2 (now) | Phase 4+5+cluster can add limiters without touching security-critical InstanceHardLimiter |

**Deprecated/outdated:**
- **StoreLimitsBuilder for multi-tenant:** Only provides per-Store static limits. Cannot track tenant-level aggregate quotas. Use custom `ResourceLimiter` trait impls instead. [CITED: PITFALLS.md -- Pitfall 4]
- **`with_default_exporter()` (opentelemetry):** Deprecated global provider. Use explicit `SdkTracerProvider`/`SdkMeterProvider` when Phase 9 adds observability. Not relevant to Phase 2. [CITED: CLAUDE.md -- What NOT to Use]
- **Activating WASI network by default:** WASI preview1 allows TCP/UDP by default. For jadepaw's security model, network must be capability-gated via `socket_addr_check` callback on `WasiCtxBuilder`. [CITED: docs.rs wasmtime-wasi WasiCtxBuilder]

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `PoolingAllocatorConfig::max_memory_size(u64)` is the correct method to set per-slot memory pool size in wasmtime 45.0. The docs.rs page for PoolingAllocatorConfig returned 404 for wasmtime 45.0.0 -- exact method signature not verified. | Standard Stack / Pitfalls | Pool slots sized incorrectly -- either virtual address space wasted (if too large) or instances can't allocate full 64MB (if too small). Should be verified with `cargo doc --open` or by checking the wasmtime source before implementation. |
| A2 | `PoolingAllocatorConfig::default()` provides reasonable defaults for `total_memories`, `total_tables`, `total_core_instances` that can support 10k+ concurrent sessions. | Architecture Patterns | May need explicit `.total_core_instances(10240)` or similar if default is lower. Verify against wasmtime source or runtime testing. |
| A3 | `Engine::weak()` returns `EngineWeak` which has `increment_epoch()` and `upgrade()` methods. | Code Examples / Epoch Ticker | If `increment_epoch()` is only on `Engine` not `EngineWeak`, the epoch ticker pattern would need adjustment (hold `Arc<Engine>` instead). The docs.rs Engine page confirms `EngineWeak` and `Engine::weak()` exist, and `increment_epoch` is available with `target_has_atomic=64`. |
| A4 | The pooling-allocator, async, cranelift, and runtime features are all available and compatible in wasmtime 45.0 with `default-features = false`. | Standard Stack | Feature mismatch could cause compilation errors. All four features are listed as enabled by default in the wasmtime docs.rs index, so explicit opt-in with default-features=false should work. |
| A5 | `Linker::instantiate_pre` with `InstancePre<SessionState>` and `InstancePre::instantiate_async` are compatible with PoolingAllocator. | Architecture Patterns | If InstancePre doesn't work with PoolingAllocator, would need to fall back to `Linker::instantiate_async`. The InstancePre docs confirm it's created via Linker::instantiate_pre and has instantiate_async. |

## Open Questions (RESOLVED)

1. **PoolingAllocatorConfig exact API surface for wasmtime 45.0** — **RESOLVED**
   - What we know: `Config::allocation_strategy(InstanceAllocationStrategy::Pooling { config })` exists. `PoolingAllocatorConfig::default()` exists and has `max_memory_size` method (referenced in D-09). Docs.rs returned 404 for the detailed struct page.
   - What's unclear: Exact method names for `total_memories`, `total_core_instances`, `table_elements`, `memory_pages`. Whether `max_unused_warm_slots` is a config option or an internal detail.
   - Resolution: Run `cargo doc --open --package wasmtime` locally to verify exact API before implementation. This is a one-time check that takes <2 minutes. Plan 02-01 Task 2 `<interfaces>` documents expected method names; executor verifies them via `cargo doc` before writing code.

2. **WasiCtx vs WasiP1Ctx for wasm32-wasi guest modules** — **RESOLVED**
   - What we know: Guest modules compile to `wasm32-wasi` target. WASI preview1 is the stable interface. `WasiCtxBuilder::build_p1()` produces a `WasiP1Ctx` for preview1.
   - What's unclear: Whether `WasiP1Ctx` integrates cleanly with `Linker<SessionState>` and async stores. Whether the `p1` feature is required.
   - Resolution: Use preview1 (`WasiCtxBuilder::build_p1()`) with the `p1` feature. This is the stable, well-documented path. Preview2/component model is explicitly out of scope (D-03). WASI context setup is deferred to a follow-up plan in this phase — guest modules for Phase 2 only need the `"jadepaw"` host function imports.

3. **64MB memory cap: is 64MB the guest module's linear memory limit, or the Store's total memory budget (including wasmtime overhead)?** — **RESOLVED**
   - What we know: The requirements say 64MB/instance. StoreLimits and ResourceLimiter control only WebAssembly linear memory growth. wasmtime's own internal structures (VMContext, instance handle, wasm stack) are not counted.
   - What's unclear: What the actual per-instance WASM overhead is with wasmtime 45.0 + PoolingAllocator. The total memory per session could be 64MB (Wasm) + X MB (host overhead).
   - Resolution: Measure total process RSS per session in the stress test (success criterion 5: 1,000 concurrent sessions). The 64MB cap applies to guest linear memory only (enforced by InstanceHardLimiter). Plan 02-03 Task 2 stress test measures actual RSS and reports overhead. If overhead per session is significant (e.g., >2MB), document it separately.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust (rustc) | Phase build & test | Yes | 1.95.0 (2026-04-14) | -- |
| Cargo | Build system | Yes | 1.95.0 | -- |
| tokio runtime | Async wasmtime execution | Yes | 1.52 (workspace) | -- |
| wasmtime crate | Core Wasm runtime | Yes | 45.0 (workspace) | -- |
| cargo nextest | Test runner | No | -- | `cargo test --workspace` (slower but functional) |

**Missing dependencies with no fallback:** none
**Missing dependencies with fallback:**
- cargo nextest: not installed. `cargo test --workspace` works as a fallback for running tests.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `#[tokio::test(flavor = "multi_thread")]` |
| Config file | `.config/nextest.toml` (for nextest if installed) or standard cargo test |
| Quick run command | `cargo test -p jadepaw-wasm --lib` |
| Full suite command | `cargo test -p jadepaw-wasm` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SEC-01 | Store-per-session isolation -- data from session A not visible in session B | integration | `cargo test -p jadepaw-wasm test_store_isolation -- --nocapture` | No -- Wave 0 |
| SEC-02 | Guest exceeding 64MB memory allocation is terminated with clear error | integration | `cargo test -p jadepaw-wasm test_memory_hard_cap -- --nocapture` | No -- Wave 0 |
| SEC-02 | Fuel exhaustion (infinite loop detection) terminates with trap | integration | `cargo test -p jadepaw-wasm test_fuel_exhaustion -- --nocapture` | No -- Wave 0 |
| SEC-02 | Epoch interruption triggers async yield | integration | `cargo test -p jadepaw-wasm test_epoch_yield -- --nocapture` | No -- Wave 0 |
| SEC-03 | Path traversal (`../../../etc/passwd`) rejected before tool runs | unit (host fn) | `cargo test -p jadepaw-wasm test_path_traversal_rejected -- --nocapture` | No -- Wave 0 |
| SEC-03 | Valid path within sandbox root accepted | unit (host fn) | `cargo test -p jadepaw-wasm test_valid_path_accepted -- --nocapture` | No -- Wave 0 |
| SEC-04 | Tool not in capability whitelist rejected with permission error | integration | `cargo test -p jadepaw-wasm test_capability_denied -- --nocapture` | No -- Wave 0 |
| SEC-04 | Tool in whitelist allowed through | integration | `cargo test -p jadepaw-wasm test_capability_allowed -- --nocapture` | No -- Wave 0 |
| Stress | 1,000 concurrent sessions each stay within 64MB cap | stress/smoke | `cargo test -p jadepaw-wasm test_concurrent_sessions_stress -- --nocapture --ignored` | No -- Wave 0 |
| D-01 | HostFunctions trait is CI-verifiable (additive-only, backwards compat) | unit | `cargo test -p jadepaw-core test_host_functions_trait -- --nocapture` | No -- Wave 0 |
| D-07 | InstanceHardLimiter returns Err() on >64MB (Store poisoned) | unit | `cargo test -p jadepaw-wasm test_instance_hard_limit_trap -- --nocapture` | No -- Wave 0 |
| D-07 | TenantQuotaLimiter returns Ok(false) on budget exceeded (recoverable) | unit | `cargo test -p jadepaw-wasm test_tenant_quota_recoverable -- --nocapture` | No -- Wave 0 |
| D-09 | Fuel metering enabled at Engine level (Config::consume_fuel = true) | unit | `cargo test -p jadepaw-wasm test_fuel_enabled -- --nocapture` | No -- Wave 0 |
| D-09 | Epoch interruption enabled at Engine level (Config::epoch_interruption = true) | unit | `cargo test -p jadepaw-wasm test_epoch_enabled -- --nocapture` | No -- Wave 0 |
| D-10 | InstanceCapabilities struct in jadepaw-core with all required fields | unit | `cargo test -p jadepaw-core test_capabilities_struct -- --nocapture` | No -- Wave 0 |
| D-12 | Default-deny: unregistered capability returns false | unit | `cargo test -p jadepaw-wasm test_default_deny -- --nocapture` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p jadepaw-wasm --lib`
- **Per wave merge:** `cargo test -p jadepaw-wasm`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/jadepaw-wasm/tests/` -- integration tests covering isolation, memory cap, capability enforcement
- [ ] `crates/jadepaw-core/tests/` -- trait and struct tests
- [ ] Test fixtures: a minimal `guest_module.wasm` compiled to `wasm32-wasi` that exercises host function imports
- [ ] Test helper: `build_test_engine()` factory function with pooling+fuel+epoch config for test reuse
- [ ] `crates/jadepaw-core/src/host_functions.rs` -- HostFunctions trait definition
- [ ] `crates/jadepaw-core/src/capabilities.rs` -- InstanceCapabilities, PathPattern, DomainPattern types

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Phase 6 (Skill System) handles tenant authentication |
| V3 Session Management | Yes | Store-per-session with SessionState. Each Store is an independent security boundary. SessionId tracked in DashMap. Store dropped on session end -- zero data residue. |
| V4 Access Control | Yes | Capability whitelist (D-10/D-12): default deny on tool execution, file access, network access. Every host function checks `caller.data().can_*()` at entry before side effects. |
| V5 Input Validation | Yes | Guest-provided paths normalized, canonicalized, sandbox-prefix-verified. Guest memory (ptr, len) bounds-checked before access. All host function parameters validated. |
| V6 Cryptography | Yes | `path.canonicalize()` resolves symlinks. UUID v7 for SessionId (time-ordered, database-friendly). No custom cryptography -- all from stdlib and uuid crate. |
| V7 Error Handling & Logging | Partially | Error enums differentiate `CapabilityDenied`, `TrapError`, `PathValidationError`. Audit log entries created for denied capability checks. Full logging in Phase 9. |
| V13 API Security | Yes | Guest-host interface is the API boundary. HostFunctions trait is additive-only (D-01). Capability checks at every API entry point. No guest code has direct access to host resources. |

### Known Threat Patterns for wasmtime/Wasm Sandbox

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal via `../` in file paths | Tampering | `canonicalize()` + sandbox root prefix check. Reject before any I/O. |
| Symlink escape from sandbox root | Elevation of Privilege | `canonicalize()` resolves symlinks. If symlink points outside sandbox, prefix check fails. |
| Infinite loop in Wasm guest | Denial of Service | Fuel metering: `Store::set_fuel(N)` provides hard upper bound. Epoch: cooperative yield at deadline. |
| Memory exhaustion via repeated `memory.grow` | Denial of Service | ResourceLimiter::memory_growing returns Err() at 64MB hard cap. TenantQuotaLimiter returns Ok(false) at aggregate budget. |
| Guest reading stale data from previous session | Information Disclosure | Store-per-session (D-04). Store is dropped on session end. PoolingAllocator ensures memory slots are zeroed before reuse. |
| Guest calling unauthorized tool | Elevation of Privilege | `caller.data().can_call_tool(id)` at entry of every tool host function. Default deny (empty whitelist). |
| Guest accessing unauthorized network domains | Information Disclosure | `caller.data().can_access_domain(domain)` at entry of network host functions. Reject private/loopback IPs (SSRF prevention). |
| Memory corruption via invalid guest (ptr, len) | Tampering | Bounds-check all guest memory accesses against `memory.data_size()`. wasmtime validates ptr+len within linear memory. |
| Host function blocks thread during Wasm call | Denial of Service | All I/O host functions must be async (`func_wrap_async`). Use `tokio::time::timeout` for external calls. |

## Sources

### Primary (HIGH confidence)
- docs.rs/wasmtime/45.0.0/wasmtime/struct.Config.html -- `consume_fuel`, `epoch_interruption`, `allocation_strategy`, `async_support` APIs. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/trait.ResourceLimiter.html -- `ResourceLimiter` trait, `memory_growing`, `table_growing`, return semantics (Ok(true)/Ok(false)/Err()). [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.Linker.html -- `func_wrap_async`, `func_wrap`, `instantiate_pre`, `Linker::module` APIs. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.InstancePre.html -- `instantiate`, `instantiate_async`, `module()`, `Clone + Send + Sync` trait impls. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.Store.html -- `set_fuel`, `fuel_async_yield_interval`, `set_epoch_deadline`, `epoch_deadline_async_yield_and_update`, `limiter`, `data`, `data_mut`, `get_fuel`. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.Engine.html -- `increment_epoch`, `weak()`, `EngineWeak`, `tls_eager_initialize()`. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.Caller.html -- `data()`, `data_mut()`, `get_export()`. [VERIFIED]
- docs.rs/wasmtime/latest/wasmtime/struct.StoreLimitsBuilder.html -- `memory_size`, `instances`, `tables`, `memories`, `table_elements`, `trap_on_grow_failure`, defaults. [VERIFIED]
- docs.rs/wasmtime-wasi/latest/wasmtime_wasi/struct.WasiCtxBuilder.html -- `build()`, `build_p1()`, `preopened_dir()`, `inherit_stdio()`, `socket_addr_check()`. [VERIFIED]
- docs.rs/wasmtime/45.0.0/wasmtime/index.html -- Feature flags: pooling-allocator, async, cranelift, runtime (all enabled by default). [VERIFIED]
- docs/jadepaw_discussion.md -- Wasm isolation model, instance pool design (Section 3.1), security model (Section 4), path validation. [VERIFIED: primary project design document]
- .planning/research/PITFALLS.md -- 8 documented pitfalls with wasmtime, all applicable to Phase 2. [VERIFIED: project research]
- .planning/phases/02-wasm-isolation-core/02-CONTEXT.md -- User decisions D-01 through D-12. [VERIFIED: user decisions]
- .planning/phases/02-wasm-isolation-core/02-DISCUSSION-LOG.md -- Alternatives considered for each decision area. [VERIFIED: discussion trail]

### Secondary (MEDIUM confidence)
- crates.io search `wasmtime` -- confirmed v45.0.0 is current published version. [VERIFIED]

### Tertiary (LOW confidence)
- docs.rs PoolingAllocatorConfig detailed page -- returned 404 for wasmtime 45.0.0. Exact builder API (total_memories, total_core_instances, table_elements, memory_pages, max_unused_warm_slots) not verified. [ASSUMED -- A1, A2]
- docs.rs InstanceAllocationStrategy detailed page -- returned 404. Constructor for `Pooling { config }` variant not directly verified but confirmed via Config::allocation_strategy docs reference. [VERIFIED via Config docs]

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all crates verified on crates.io or already in workspace Cargo.toml
- Architecture: HIGH -- patterns verified against wasmtime 45.0 official docs (ResourceLimiter trait, Store, Linker, InstancePre, Caller APIs all confirmed)
- Pitfalls: HIGH -- cross-referenced with PITFALLS.md (primary project research) and wasmtime official docs

**Research date:** 2026-05-30
**Valid until:** 2026-07-30 (wasmtime 45.0 is stable; major API changes unlikely within 60 days)

**What was not-researched:**
- Exact PoolingAllocatorConfig builder method signatures (A1, A2) -- docs.rs returned 404. Recommend local `cargo doc` verification before implementation.
- wasm32-wasi guest module compilation toolchain -- deferred to Phase 6 (Skill System) where guest modules are built. Phase 2 only needs a test fixture .wasm file.
- WasiView trait integration with Store<SessionState> -- the WasiCtxBuilder docs did not mention WasiView. This requires further investigation when implementing WASI context setup.