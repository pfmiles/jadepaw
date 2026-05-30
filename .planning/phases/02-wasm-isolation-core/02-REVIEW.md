---
phase: 02-wasm-isolation-core
reviewed: 2026-05-30T17:30:00Z
depth: standard
files_reviewed: 32
files_reviewed_list:
  - crates/jadepaw-core/Cargo.toml
  - crates/jadepaw-core/src/capabilities.rs
  - crates/jadepaw-core/src/error.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/types.rs
  - crates/jadepaw-core/tests/capabilities.rs
  - crates/jadepaw-core/tests/types.rs
  - crates/jadepaw-wasm/Cargo.toml
  - crates/jadepaw-wasm/src/capability/mod.rs
  - crates/jadepaw-wasm/src/engine.rs
  - crates/jadepaw-wasm/src/epoch.rs
  - crates/jadepaw-wasm/src/host/filesystem.rs
  - crates/jadepaw-wasm/src/host/logging.rs
  - crates/jadepaw-wasm/src/host/mod.rs
  - crates/jadepaw-wasm/src/host/network.rs
  - crates/jadepaw-wasm/src/lib.rs
  - crates/jadepaw-wasm/src/limits/instance_hard.rs
  - crates/jadepaw-wasm/src/limits/mod.rs
  - crates/jadepaw-wasm/src/limits/tenant_quota.rs
  - crates/jadepaw-wasm/src/linker.rs
  - crates/jadepaw-wasm/src/path.rs
  - crates/jadepaw-wasm/src/pool.rs
  - crates/jadepaw-wasm/src/session.rs
  - crates/jadepaw-wasm/tests/capability.rs
  - crates/jadepaw-wasm/tests/engine_smoke.rs
  - crates/jadepaw-wasm/tests/fixtures/noop.wat
  - crates/jadepaw-wasm/tests/fixtures/tool_caller.wat
  - crates/jadepaw-wasm/tests/limits.rs
  - crates/jadepaw-wasm/tests/path_validation.rs
  - crates/jadepaw-wasm/tests/pool.rs
  - crates/jadepaw-wasm/tests/stress_concurrent.rs
findings:
  critical: 3
  warning: 4
  info: 2
  total: 9
status: issues_found
---

# Phase 02: Code Review Report

**Reviewed:** 2026-05-30T17:30:00Z
**Depth:** standard
**Files Reviewed:** 32
**Status:** issues_found

## Summary

Phase 02 (Wasm Isolation Core) delivers per-session Wasm sandboxing with hardware-level tenant isolation, engine factory configuration, host functions with capability enforcement, resource limiting, path validation/sandboxing, and an instance pool with lazy instantiation.

The architecture is sound and follows the design constraints (D-01 to D-12) well. The default-deny capability model, multi-layered defense sequence (bounds check -> capability check -> path validation -> I/O), and wasmtime safety features (fuel + epoch + pooling allocator) are correctly implemented.

Three critical bugs and four warnings were identified. The most concerning is a budget counter leak in `TenantQuotaLimiter` that permanently inflates the counter when the inner `InstanceHardLimiter` rejects memory growth — causing future legitimate allocations to be incorrectly denied.

---

## Critical Issues

### CR-01: TenantQuotaLimiter budget counter leak on inner memory_growing rejection

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs:83-88`
**Issue:** In `TenantQuotaLimiter::memory_growing`, the `fetch_add(delta)` on the shared budget counter is called **before** delegating to `self.inner.memory_growing(...)`. If the inner `InstanceHardLimiter` returns `Err()` (hard cap exceeded), the budget counter has already been incremented — but the growth was rejected. The delta is permanently leaked, causing the tenant budget counter to drift upward. This causes subsequent legitimate memory growth requests to be incorrectly denied with `Ok(false)` because the counter appears to exceed the budget.
**Fix:**
```rust
fn memory_growing(
    &mut self,
    current: usize,
    desired: usize,
    maximum: Option<usize>,
) -> wasmtime::Result<bool> {
    let delta = desired.saturating_sub(current);

    let used = self.tenant_budget_used.load(Ordering::Relaxed);
    if used + delta > self.tenant_budget_max {
        return Ok(false);
    }

    // Delegate to inner FIRST — only commit the budget if inner approves
    let inner_result = self.inner.memory_growing(current, desired, maximum)?;
    // inner_result is always Ok(true) here (Err would have propagated via ?)
    // Now it's safe to commit the budget:
    self.tenant_budget_used
        .fetch_add(delta, Ordering::Relaxed);
    Ok(inner_result)
}
```

### CR-02: TenantQuotaLimiter budget counter leak on inner table_growing rejection

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs:91-105`
**Issue:** Same pattern as CR-01, but in `table_growing`. The `fetch_add` on the budget counter is called before `self.inner.table_growing(...)`. If the inner limiter rejects, the counter is inflated.
**Fix:**
```rust
fn table_growing(
    &mut self,
    current: usize,
    desired: usize,
    maximum: Option<usize>,
) -> wasmtime::Result<bool> {
    let delta = desired.saturating_sub(current);
    let used = self.tenant_budget_used.load(Ordering::Relaxed);
    if used + delta > self.tenant_budget_max {
        return Ok(false);
    }
    // Delegate first, commit budget only on success
    let inner_result = self.inner.table_growing(current, desired, maximum)?;
    self.tenant_budget_used
        .fetch_add(delta, Ordering::Relaxed);
    Ok(inner_result)
}
```

### CR-03: `EngineFactory::build` missing `async_support(true)` on wasmtime Config

**File:** `crates/jadepaw-wasm/src/engine.rs:41-61`
**Issue:** The `EngineFactory::build()` method creates a `Config` and enables `consume_fuel`, `epoch_interruption`, and Cranelift optimizations, but never calls `config.async_support(true)`. All host functions are registered via `func_wrap_async` and return `Box<dyn Future>`. In wasmtime 45.0, `func_wrap_async` validates that async support is enabled on the engine — if it is not, the registration will fail at call time with a wasmtime trap or error. The project's own design documentation (engine.rs comment block line 13, CLAUDE.md section on `wasmtime 38.x`) explicitly lists `async_support(true)` as required. While the current test suite may pass (wasmtime 45 may have relaxed this check), the explicit configuration is required for correct behavior and maintainability.
**Fix:**
```rust
// In EngineFactory::build(), add after config.epoch_interruption(true):
config.async_support(true);
```

---

## Warnings

### WR-01: i32 pointer arithmetic overflow risk in host function bounds checks

**File:** `crates/jadepaw-wasm/src/host/filesystem.rs:65, 165, 180-181`, `crates/jadepaw-wasm/src/host/logging.rs:56, 74`, `crates/jadepaw-wasm/src/host/network.rs:67, 99`
**Issue:** Multiple host functions compute `(path_ptr + path_len) as usize` where both operands are `i32`. If a malicious guest passes values whose sum exceeds `i32::MAX`, the addition overflows. In Rust debug mode this panics (crashing the host process), and in release mode it wraps to a negative value that gets cast to a large `usize` — which is then caught by the `> mem_size` bounds check. The crash-in-debug issue means that running debug builds in development could be disrupted by a malicious or buggy guest module.

This pattern appears in `file_read_host_fn` (lines 65, 180-181), `file_write_host_fn` (line 165, 180-181), `log_message_host_fn` (lines 56, 74), and `http_request_host_fn` (lines 67, 99).
**Fix:**
```rust
// Instead of (ptr + len) as usize, use:
let path_start = path_ptr as usize;
let path_len_usize = path_len as usize;
let path_end = path_start.checked_add(path_len_usize)
    .unwrap_or(usize::MAX); // any overflow -> guaranteed out of bounds
```

### WR-02: Epoch ticker thread cannot exit via EngineWeak when Engine is leaked

**File:** `crates/jadepaw-wasm/src/epoch.rs:78`
**Issue:** The epoch ticker thread clones the `Engine` (`engine_clone`) and holds it inside the thread. The `engine_weak.upgrade().is_none()` check on line 78 is intended to detect when the Engine has been dropped outside the thread and exit gracefully. However, because `engine_clone` (a clone of the Engine) lives in the thread, it keeps the Engine's reference count above zero. The `engine_weak.upgrade()` will **never** return `None` as long as the thread is running. The only way the thread exits is via the `stop_rx` channel (line 73), which requires `EpochTickerGuard::drop()` to be called.

Under normal usage (guard dropped at scope exit), this works correctly. However, the comment on lines 17-19 is misleading — the cloned Engine does keep the Engine alive past the main Engine's lifetime until the ticker thread exits. If the `EpochTickerGuard` were leaked (e.g., via `std::mem::forget`), both the thread and the Engine would leak.
**Fix:** Update the comments to reflect the actual behavior, or remove the dead `engine_weak.upgrade()` check to simplify the code path (the stop signal is the canonical shutdown mechanism). Alternatively, use `EngineWeak` for the increment call too and rely on `upgrade()` failing as the exit signal, eliminating the need for `engine_clone`:

```rust
pub fn start_epoch_ticker(engine: &Engine) -> EpochTickerGuard {
    let engine_weak = engine.weak();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();

    let handle = thread::spawn(move || {
        let tick = Duration::from_millis(1);
        loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            if let Some(engine) = engine_weak.upgrade() {
                engine.increment_epoch();
                // engine dropped here — reference released
            } else {
                break; // Engine was dropped, exit
            }
            thread::sleep(tick);
        }
    });
    // ...
}
```

### WR-03: Path normalization does not truncate overly long paths

**File:** `crates/jadepaw-wasm/src/path.rs:41-67`
**Issue:** `normalize_path` accepts an arbitrary-length input string and produces a `PathBuf` without any length limit. While `PathBuf` internally limits to OS-specific maximum path lengths, a guest module could pass a path with millions of characters, causing `normalize_path` to allocate a large `Vec` and `PathBuf`. This is a minor DoS vector (memory pressure, not a crash). The capability and path validation checks would subsequently fail or the OS would reject the path, but the allocation cost happens before those checks.
**Fix:** Add a length cap early in `normalize_path`:
```rust
pub fn normalize_path(path: &str) -> PathBuf {
    // Guard against excessive path lengths
    const MAX_PATH_LEN: usize = 4096;
    if path.len() > MAX_PATH_LEN {
        // Return a sentinel that will fail subsequent validation
        return PathBuf::from("..");
    }
    // ... rest of function
}
```

### WR-04: `capacity()` uses `Semaphore::available_permits()` which has documented accuracy caveats

**File:** `crates/jadepaw-wasm/src/pool.rs:240-242`
**Issue:** The `capacity()` method computes `self.semaphore.available_permits() + self.active_count()`. Tokio's `Semaphore::available_permits()` documentation states: "This is a best-effort operation and has no guarantees about accuracy." For a fair semaphore under normal conditions this should be accurate, but the API contract warns against relying on it for correctness guarantees. The `capacity()` method is used only in tests (pool.rs tests, stress_concurrent.rs) where it functions as an informational check rather than a correctness guarantee, but it's a public API that could mislead callers.
**Fix:** Store the `max_concurrent` value as a field in `InstancePool` and return it directly for `capacity()`, eliminating the reliance on semaphore state:
```rust
pub struct InstancePool {
    // ...
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,  // store the configured capacity
    // ...
}

pub fn capacity(&self) -> usize {
    self.max_concurrent
}
```

---

## Info

### IN-01: `TenantQuotaLimiter::new_with_budget` duplicates `new` exactly

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs:58-64`
**Issue:** The `new_with_budget` constructor is documented as "Lower-level constructor that accepts a pre-measured budget in bytes" but its implementation simply delegates to `Self::new(budget_max_mb, tenant_budget_used, inner)` — identical behavior to the public `new` constructor (line 43-53), which also takes `budget_max_mb: u32`. The `#[doc(hidden)]` attribute hides it from docs but the function is dead weight with no distinct behavior.
**Fix:** Either make `new_with_budget` accept `budget_max_bytes: usize` directly (to fulfill its documented purpose of accepting pre-measured bytes) or remove it and use `new` directly in tests.

### IN-02: Misleading comment in `normalize_path` test about parent traversal behavior

**File:** `crates/jadepaw-wasm/src/path.rs:228-235`
**Issue:** The test comment on lines 229-233 contains a line-by-line trace that is correct but confusingly phrased. The comment starts with "But actually: ../.. means..." in a way that suggests it's correcting an earlier guess, then traces correctly through to the right answer. The intermediate scratch-work style comments should be cleaned up.
**Fix:** Replace lines 229-233 with a clean explanation:
```rust
    // "../../..": [] -> push ".." -> [".."] -> pop ".." -> []
    //             -> push ".." -> [".."]
```

---

_Reviewed: 2026-05-30T17:30:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_