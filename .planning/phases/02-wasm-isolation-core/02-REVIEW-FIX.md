---
phase: 02-wasm-isolation-core
fixed_at: 2026-05-30T18:00:00Z
review_path: .planning/phases/02-wasm-isolation-core/02-REVIEW.md
iteration: 1
findings_in_scope: 7
fixed: 6
skipped: 1
status: partial
---

# Phase 02: Code Review Fix Report

**Fixed at:** 2026-05-30T18:00:00Z
**Source review:** .planning/phases/02-wasm-isolation-core/02-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 7 (3 Critical, 4 Warning)
- Fixed: 6
- Skipped: 1

## Fixed Issues

### CR-01: TenantQuotaLimiter budget counter leak on inner memory_growing rejection

**Files modified:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs`
**Commit:** 8e1fcb3
**Applied fix:** Reordered `memory_growing`: delegate to `self.inner.memory_growing(...)` first (propagating `Err` via `?`), then commit `fetch_add` on the tenant budget counter only after the inner limiter approves the growth. Prevents the budget counter from being permanently inflated when the inner `InstanceHardLimiter` rejects memory growth.

### CR-02: TenantQuotaLimiter budget counter leak on inner table_growing rejection

**Files modified:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs`
**Commit:** 8e1fcb3
**Applied fix:** Same fix as CR-01 applied to `table_growing`: delegate to `self.inner.table_growing(...)` first, commit `fetch_add` only on success. Combined with CR-01 in a single commit since both are in the same `impl ResourceLimiter` block.

### WR-01: i32 pointer arithmetic overflow risk in host function bounds checks

**Files modified:** `crates/jadepaw-wasm/src/host/filesystem.rs`, `crates/jadepaw-wasm/src/host/logging.rs`, `crates/jadepaw-wasm/src/host/network.rs`
**Commit:** b08d721
**Applied fix:** Replaced all `(ptr + len) as usize` expressions with `ptr_start.checked_add(len_usize).unwrap_or(usize::MAX)`. This prevents i32 overflow when a malicious guest passes pointer-length pairs whose sum exceeds i32::MAX. In debug mode, the raw addition would panic (crashing the host process); with `checked_add`, overflow is caught gracefully as an out-of-bounds rejection via the existing `if end > mem_size` check.

### WR-02: Epoch ticker thread cannot exit via EngineWeak when Engine is leaked

**Files modified:** `crates/jadepaw-wasm/src/epoch.rs`
**Commit:** d58724a
**Applied fix:** Removed the separate `engine_clone` and now uses `engine_weak.upgrade()` for both liveness detection and `increment_epoch()`. The thread no longer holds an `Engine` reference that would keep the `EngineWeak` from ever returning `None`. When the Engine is dropped, `upgrade()` returns `None` and the thread exits gracefully without requiring the stop channel signal. Updated module-level documentation accordingly.

### WR-03: Path normalization does not truncate overly long paths

**Files modified:** `crates/jadepaw-wasm/src/path.rs`
**Commit:** 3f244f2
**Applied fix:** Added a `MAX_PATH_LEN` constant (4096) at the top of `normalize_path`. Paths exceeding this length return a `".."` sentinel `PathBuf` that will be caught by the subsequent sandbox validation in `validate_sandbox_path`, preventing large allocations from arbitrarily long guest-provided path strings.

### WR-04: `capacity()` uses `Semaphore::available_permits()` which has documented accuracy caveats

**Files modified:** `crates/jadepaw-wasm/src/pool.rs`
**Commit:** 758c856
**Applied fix:** Added a `max_concurrent: usize` field to `InstancePool`, stored from the configuration at construction time. The `capacity()` method now returns this stored field directly instead of computing `self.semaphore.available_permits() + self.active_count()`. Tokio's `Semaphore::available_permits()` is documented as best-effort with no accuracy guarantees; the stored field eliminates this unreliability.

## Skipped Issues

### CR-03: `EngineFactory::build` missing `async_support(true)` on wasmtime Config

**File:** `crates/jadepaw-wasm/src/engine.rs:41-61`
**Reason:** In wasmtime 45.0 (the version actually used by this codebase), `Config::async_support()` is deprecated with the message "no longer has any effect". The project already uses wasmtime 45.0 (`crates/jadepaw-wasm/Cargo.toml`), not 38.0 as the design documentation assumed. Adding `config.async_support(true)` would introduce a compiler warning for no benefit since async support is the default and always-on behavior in wasmtime 45+. The code compiles and functions correctly without this line.

**Original issue:** `EngineFactory::build()` creates a `Config` and enables `consume_fuel`, `epoch_interruption`, and Cranelift optimizations, but never calls `config.async_support(true)`. All host functions are registered via `func_wrap_async` and return `Box<dyn Future>`. In wasmtime 45.0, `func_wrap_async` validates that async support is enabled on the engine -- if it is not, the registration will fail at call time with a wasmtime trap or error. The project's own design documentation (engine.rs comment block line 13, CLAUDE.md section on `wasmtime 38.x`) explicitly lists `async_support(true)` as required. While the current test suite may pass (wasmtime 45 may have relaxed this check), the explicit configuration is required for correct behavior and maintainability.

---

_Fixed: 2026-05-30T18:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_