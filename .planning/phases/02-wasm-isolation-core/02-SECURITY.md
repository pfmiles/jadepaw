---
phase: 02
slug: wasm-isolation-core
status: verified
threats_open: 0
asvs_level: 1
created: 2026-05-30
---

# Phase 02 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Guest Wasm module -> Host | Guest-provided data enters host via function arguments (ptr, len). Untrusted. | Guest memory pointers, path strings, operation parameters |
| Guest Wasm module -> Store | Guest executes within Store's linear memory. ResourceLimiter is enforcement point. | Memory allocation requests, fuel consumption |
| Engine -> OS Memory | PoolingAllocator reserves virtual address space at Engine creation. | Memory slots (64MB each), instance handles |
| Guest -> Host function entry | Guest passes (ptr, len) to host functions. Bounds-checked before memory access. | Guest memory reads/writes |
| Guest -> Path validation | Guest-provided path string is untrusted. normalize + canonicalize + sandbox prefix check. | Filesystem paths |
| Guest -> Capability check | Guest-provided operation is checked against per-session InstanceCapabilities before any side effect. | Operation type + target resource |
| Guest -> File I/O | Path must pass both capability whitelist AND sandbox boundary check before tokio::fs access. | File contents |
| Guest -> Network | Domain checked against can_network_to whitelist before any outbound connection. | URL, HTTP method, headers, body |
| Session A -> Session B | Store-per-session isolation. SessionHandle drop destroys Store. Zero data residue. | Session data, guest memory contents |
| Pool -> OS Resources | Semaphore bounds max concurrent sessions. DashMap provides O(1) session lookup. | Session handles, semaphore permits |

---

## Threat Register

### Plan 02-01: Engine, ResourceLimiter, Core Types

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-02-01 | Denial of Service | wasmtime Store | mitigate | Fuel metering with `set_fuel(1_000_000)` per turn. Infinite loops trap with clear error. | closed |
| T-02-02 | Denial of Service | wasmtime Store | mitigate | Epoch interruption with 1ms ticker. `epoch_deadline_async_yield_and_update(100)` per session. | closed |
| T-02-03 | Denial of Service | ResourceLimiter | mitigate | `InstanceHardLimiter` returns `Err()` at 64MB (Store poisoned). `PoolingAllocationConfig.max_memory_size=64MB` matches. `TenantQuotaLimiter` returns `Ok(false)` for aggregate budget (recoverable). | closed |
| T-02-04 | Information Disclosure | Store lifecycle | mitigate | Fresh `Store::new()` per session, dropped on session end. No Store reuse. PoolingAllocator zeros memory slots before reuse. | closed |
| T-02-05 | Denial of Service | PoolingAllocator | mitigate | `max_memory_size=64MB` (not default 4GiB). Explicit `total_core_instances` supports 10k+ concurrent instances on 64-bit. | closed |
| T-02-06 | Denial of Service | Epoch ticker | mitigate | `EngineWeak` prevents holding Engine alive. Ticker exits when Engine dropped. `EpochTickerGuard` joins thread on drop. | closed |
| T-02-SC | Tampering | wasmtime crate | accept | wasmtime 45.0.0 verified on crates.io. Bytecode Alliance. Formal security disclosure process. | closed |

### Plan 02-02: Host Mediation, Capability Enforcement, Path Validation

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-02-07 | Elevation of Privilege | host/filesystem.rs | mitigate | `can_read_file()` checked before every `file_read`. `can_write_file()` checked before every `file_write`. Default deny (empty whitelist). | closed |
| T-02-08 | Tampering | path.rs | mitigate | `validate_sandbox_path`: normalize `..`/`.`, `Path::canonicalize` resolves symlinks, `starts_with(sandbox_root)` prefix check. Traversal rejected before any I/O. | closed |
| T-02-09 | Information Disclosure | Guest memory access | mitigate | Every (ptr, len) from guest bounds-checked against `Memory::data_size(&caller)` before reading/writing. wasmtime validates ptr+len stays within linear memory. | closed |
| T-02-10 | Elevation of Privilege | host/network.rs | mitigate | `can_access_domain()` checked before any HTTP request. Default deny (empty DomainPattern whitelist). Full network capability deferred to Phase 4. | closed |
| T-02-11 | Denial of Service | Host function blocking | mitigate | All I/O host functions use `func_wrap_async` (not `func_wrap`), executing on tokio runtime. `tokio::time::timeout` wrapper added in Phase 4. | closed |
| T-02-12 | Repudiation | host/logging.rs | mitigate | `log_message` host function always records `session_id` from `caller.data()`. Auditable even for always-allowed operations. | closed |

### Plan 02-03: Instance Pool, Session Lifecycle, Stress Test

| Threat ID | Category | Component | Disposition | Mitigation | Status |
|-----------|----------|-----------|-------------|------------|--------|
| T-02-13 | Information Disclosure | SessionHandle Drop | mitigate | Store dropped on SessionHandle drop. PoolingAllocator zeros memory slots before reuse. Session isolation test verifies no data leakage between sessions (SEC-01). | closed |
| T-02-14 | Denial of Service | Semaphore | mitigate | `max_concurrent` bounds prevent unbounded session creation. `acquire()` blocks (async backpressure) when pool exhausted. Configurable at pool creation. | closed |
| T-02-15 | Denial of Service | PoolingAllocator | mitigate | Stress test verifies 1000 concurrent sessions at 64MB each. `PoolingAllocationConfig.max_memory_size=64MB` (not 4GiB default). Failure mode: acquire fails with clear error, not OOM kill. | closed |
| T-02-16 | Tampering | InstancePre | mitigate | InstancePre is `Arc`-shared and read-only. No mutable state shared across sessions. Each `acquire()` creates a new Store with fresh SessionState. | closed |

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| R-02-01 | T-02-SC | wasmtime 45.0.0 is a Bytecode Alliance project with a formal security disclosure process. Supply chain risk accepted at this trust level. Cargo-deny and cargo-audit run in CI. | pf_miles | 2026-05-30 |
| R-02-02 | T-02-08 (TOCTOU) | `validate_sandbox_path` has a TOCTOU window between `exists()` check and `canonicalize()`/file I/O where a symlink could be created. Parent canonicalization limits the window to the filename component only. Full `openat2(RESOLVE_NO_SYMLINKS)` or equivalent mitigation deferred to Phase 4 hardening. | pf_miles | 2026-05-30 |
| R-02-03 | T-02-10 | `http_request` host function is stubbed (returns CapabilityDenied for all requests). Full HTTP implementation deferred to Phase 4. Domain validation and bounds-checking are active; no outbound network access is possible in Phase 2. | pf_miles | 2026-05-30 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-05-30 | 17 | 17 | 0 | gsd-secure-phase (automated) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-05-30