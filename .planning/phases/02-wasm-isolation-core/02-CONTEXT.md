# Phase 2: Wasm Isolation Core - Context

**Gathered:** 2026-05-30
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase builds the per-session Wasm sandbox: isolated wasmtime Stores with strict resource limits (64MB memory, Fuel+Epoch metering), a capability whitelist that defaults to deny, and host-mediated tool execution with sandboxed path validation. The guest-host communication contract is defined here as a one-way door — all subsequent phases depend on it.

**Success Criteria (from ROADMAP.md):**
1. Create a fresh wasmtime Store per session, load a guest module, execute Wasm code — Store and linear memory are destroyed on session end with no data leaking
2. Guest exceeding 64MB memory allocation is terminated with clear error; same for Fuel exhaustion (infinite loop detection) and Epoch interruption
3. Guest calling a host tool with a path like `../../../etc/passwd` is rejected before the tool runs — only paths within the tenant's designated sandbox directory are accepted
4. Guest attempting to use a tool not in its capability whitelist is rejected with a permission error before any side effects occur
5. Running 1,000 concurrent isolated sessions does not cause memory exhaustion (stress test: each session stays within its 64MB cap)

**Requirements covered:** SEC-01, SEC-02, SEC-03, SEC-04

</domain>

<decisions>
## Implementation Decisions

### Guest-Host Communication Interface
- **D-01:** Define a `HostFunctions` trait in `jadepaw-core` that catalogues every host function import. This trait is the canonical, versioned interface contract — additive-only, CI-verifiable.
- **D-02:** Implement the trait in `jadepaw-wasm` using wasmtime's `Linker::func_wrap` / `func_wrap_async` on core wasm modules (not component model). Guest modules compile to `wasm32-wasi` and import from a well-known module namespace (e.g., `"jadepaw"`).
- **D-03:** Do NOT adopt WIT/component model at this stage. The Component Model is still a W3C Phase 1 proposal — async semantics remain under active design (wasmtime 45.0's `component-model-async` requires manual FutureReader lifecycle management, and the Rust async API is nascent). The trait-based approach preserves full async support (critical for Phase 3 LLM streaming), PoolingAllocator compatibility, and sub-5ms instantiation. Migration path to WIT remains open — when (if) the Component Model reaches Phase 3-4, the trait contract can be mapped onto WIT as a compatibility shim.

### Instance Pool Lifecycle
- **D-04:** Implement lazy instantiation: pre-compile only `Module` and `InstancePre` (shared across all sessions via `Arc`). Acquire = `Store::new(engine, session_state)` + `instance_pre.instantiate_async(&mut store)`. Release = drop the Store (pooling allocator reclaims memory). No guest-side reset contract needed.
- **D-05:** Bound concurrency via `tokio::sync::Semaphore` — max concurrent instances configurable. Track active sessions in a `DashMap<SessionId, SessionHandle>`.
- **D-06:** Benchmark Store creation + instantiation latency against the 5ms P99 target before considering pre-warmed Store pool optimization. PoolingAllocator pre-allocates memory slots at Engine creation time, so Store + Instance creation is essentially slot assignment — it may already be fast enough. If profiling reveals >3ms P99, the Store Sub-pool (pre-created Stores, InstancePre instantiation on acquire) is the natural first optimization before full hot pool.

### ResourceLimiter & Termination Strategy
- **D-07:** Implement a **delegating chain** of ResourceLimiters. `InstanceHardLimiter` enforces per-instance security boundaries (64MB hard cap — `Err()` trap). `TenantQuotaLimiter` wraps `InstanceHardLimiter` and enforces tenant-level aggregate budgets — `Ok(false)` on tenant budget exceeded (guest receives -1, recoverable), then delegates to inner limiter for instance-level checks. Each limiter is independently testable.
- **D-08:** Tiered `memory_growing` semantics: `InstanceHardLimiter` returns `Err()` when 64MB per-instance hard cap is exceeded (trap, Store is terminally poisoned — this is a security boundary violation). `TenantQuotaLimiter` returns `Ok(false)` when tenant aggregate budget is exceeded (guest receives -1 from `memory.grow` — recoverable, agent can adapt). This separation of concerns keeps the security boundary auditable in isolation.
- **D-09:** Enable both Fuel metering (`Config::consume_fuel(true)`) and Epoch interruption (`Config::epoch_interruption(true)`) at Engine level from Day 1. Drive epoch ticks via a background thread per Engine. Set initial Fuel budget per agent turn via `Store::set_fuel()` (e.g., ~1,000,000 fuel units as a reasonable cap before tuning). `PoolingAllocatorConfig::max_memory_size` must match the 64MB per-instance cap so memory pool slots are correctly sized.
- **D-09a:** The delegating chain architecture is extensible by design — Phase 4 can add `ToolRateLimiter`, Phase 5 can add `SessionMemoryLimiter`, and cluster mode can swap `TenantQuotaLimiter` for `DistributedTenantQuotaLimiter` without touching the security-critical `InstanceHardLimiter`.

### Capability Enforcement API
- **D-10:** `InstanceCapabilities` struct lives in `jadepaw-core` (shared type): fields `can_read_files: Vec<PathPattern>`, `can_write_files: Vec<PathPattern>`, `can_exec_tools: Vec<ToolId>`, `can_network_to: Vec<DomainPattern>`, `max_memory_mb: u32`, `max_compute_units: u64`.
- **D-11:** Check methods on `SessionState` (the `T` in `Store<T>`) in `jadepaw-wasm`: `can_read_file(path)`, `can_write_file(path)`, `can_call_tool(id)`, `can_access_domain(domain)`. Host functions call `caller.data().can_*(...)` at entry before any side effects — enforced by code review and an integration test that verifies every registered host function accesses `caller.data()`. Define can_* methods in a dedicated `capability` module to keep the migration path to an enforcement macro clean when host function count grows beyond ~20 in Phase 4.
- **D-12:** Capability whitelist is declared at instance initialization. Default deny — if a capability is not explicitly granted, the check method returns false and the host function returns a `CapabilityDenied` error to the guest.

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Architecture & Design
- `docs/jadepaw_discussion.md` — Wasm isolation model, instance pool design (Section 3.1), security model (Section 4), capability model, path validation approach, ResourceLimiter design
- `docs/arch.mermaid` — Architecture diagram (Mermaid format)

### Wasm Runtime (Phase 2 specific)
- `crates/jadepaw-wasm/src/lib.rs` — Module-level doc describing what lives in this crate (Engine setup, instance pool, host functions, ResourceLimiter, WASI context)
- `crates/jadepaw-core/src/lib.rs` — Core types that Phase 2 depends on (SessionId, TenantId, CapabilitySet placeholder)
- `crates/jadepaw-core/Cargo.toml` — Current dependencies (serde, uuid, chrono)
- `crates/jadepaw-wasm/Cargo.toml` — Current dependencies (jadepaw-core, wasmtime 45.0, tokio)

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` §Security & Isolation — SEC-01 through SEC-04 requirements
- `.planning/ROADMAP.md` §Phase 2 — Phase goal, 5 success criteria, dependency on Phase 1
- `.planning/PROJECT.md` — Core constraints (Rust + wasmtime + tokio, Wasm hardware isolation, 64MB/instance, 10000 instances)

### Prior Phase Context
- `.planning/phases/01-project-foundation/01-CONTEXT.md` — Crate structure (D-01, D-02), dependency graph (core → wasm → bus → agent → skill → gateway → server), workspace dependencies (wasmtime 45.0, tokio 1.52), feature flag strategy

### Project State
- `.planning/STATE.md` — Current position, performance targets, pending decisions

### Research
- `.planning/research/ARCHITECTURE.md` — Per-crate file tree proposals, component responsibilities
- `.planning/research/STACK.md` — Complete technology stack, "Stack Patterns by Variant"
- `.planning/notes/mvp-core-decisions.md` — MVP core decisions

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `jadepaw-core` crate (`crates/jadepaw-core/`) — Already depends on serde, uuid, chrono. Ready to receive `HostFunctions` trait, `InstanceCapabilities` struct, `SessionId`, `TenantId`, `ToolId` types, and `PathPattern`/`DomainPattern` types.
- `jadepaw-wasm` crate (`crates/jadepaw-wasm/`) — Already depends on jadepaw-core, wasmtime 45.0, tokio. Module doc already describes Engine setup, instance pool, host functions, ResourceLimiter, and WASI context as its responsibilities.
- Workspace `Cargo.toml` — wasmtime 45.0 already in workspace dependencies with `default-features = false`. Add required features (pooling-allocator, async, cranelift) in jadepaw-wasm's Cargo.toml.

### Established Patterns
- Crate dependency graph: core → wasm (Phase 1, D-01, D-02). jadepaw-wasm only depends on jadepaw-core.
- Workspace feature flags: root `[features]` defines aggregate features (`cluster`, `single-node` as default). Sub-crate features map via `crate-name/feature` syntax.
- Hybrid feature strategy: `single-node` (default) uses in-memory components; `cluster` enables Redis for distributed state.
- Code conventions: Rust 2024 edition, `style_edition = "2024"`, `max_width = 100`, `group_imports = "StdExternalCrate"`.

### Integration Points
- Phase 3 (Agent Runtime) depends on Phase 2's Store-per-session model and guest-host interface — the `HostFunctions` trait defined here becomes the contract for agent-tool communication.
- Phase 4 (Tool System) depends on Phase 2's capability whitelist and path validation — tools register through the capability system. The delegating chain limiter architecture (D-07) will be extended with `ToolRateLimiter` in Phase 4.
- Phase 6 (Skill System) depends on Phase 2's guest-host interface — Skills compile to guest Wasm modules that import from the `"jadepaw"` namespace defined here.
- WASI context setup and preopens directory management are in jadepaw-wasm's scope — the sandbox root directory structure must be defined here.

</code_context>

<specifics>
## Specific Ideas

- Guest modules compile to `wasm32-wasi` target and import host functions from the `"jadepaw"` module namespace.
- Path validation follows the architecture doc pattern: `normalize_path()` (remove `..` and `.`) → resolve against sandbox root → verify result starts with sandbox root.
- Pooling allocator is required (not optional) — configured at Engine creation with `PoolingAllocatorConfig`. This is the only way to hit 10k+ concurrent instances.
- Instance pool uses `InstancePre` (not `Module`) for instantiation — `InstancePre` is the pre-compiled form that enables sub-ms instantiation.
- The `HostFunctions` trait and `InstanceCapabilities` struct both live in `jadepaw-core` so that `jadepaw-agent` (Phase 3) and `jadepaw-skill` (Phase 6) can reference them without depending on `jadepaw-wasm`.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 2-Wasm Isolation Core*
*Context gathered: 2026-05-30*