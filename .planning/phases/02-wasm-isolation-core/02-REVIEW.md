---
phase: 02-wasm-isolation-core
reviewed: 2026-05-30T19:00:00Z
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
  critical: 5
  warning: 0
  info: 2
  total: 7
status: issues_found
---

# Phase 02: Code Review Report (Re-review after fixes)

**Reviewed:** 2026-05-30T19:00:00Z
**Depth:** standard
**Files Reviewed:** 32
**Status:** issues_found

## Summary

This is a re-review of all 32 files after fixes were applied for six findings from the original review (CR-01, CR-02, WR-01, WR-02, WR-03, WR-04; CR-03 was intentionally skipped because `Config::async_support()` is deprecated in wasmtime 45).

All six applied fixes are verified correct. However, the re-review discovered a new critical bug in `file_read_host_fn` where a malicious guest-provided negative `buf_len` causes a host process panic. The original WR-01 fix correctly addressed i32 overflow for **read** operations (guest memory slicing), but the **write** path (writing file contents back to guest memory via `memory.write`) does not bounds-check the `buf_len` parameter before using it as a slice index, reintroducing a panic vector.

Additionally, two info-level issues from the original review remain unfixed.

---

## Critical Issues

### CR-N01: Negative `buf_len` causes host process panic in `file_read_host_fn`

**File:** `crates/jadepaw-wasm/src/host/filesystem.rs:115`
**Issue:** In `file_read_host_fn`, the `buf_len` parameter (received from guest as `i32`) is used directly in a slice operation without sanitization for negative values. When `n <= buf_len` (line 108) evaluates to `false` -- which always happens when `buf_len` is negative (e.g., `-1`) -- the else branch at line 115 executes `&contents[..buf_len as usize]`. A negative `i32` cast to `usize` wraps to a very large value (e.g., `usize::MAX`), causing `contents[..usize::MAX]` to panic because the slice index exceeds `contents.len()`.

This is a host process panic triggered by a malicious or buggy guest module. The original WR-01 fix only addressed the pointer-arithmetic overflow in the **read-from-guest-memory** path (lines 63-66), but the **write-to-guest-memory** path at line 115 was not covered.

A secondary issue exists at line 107: `contents.len() as i32` silently wraps for files larger than `i32::MAX` bytes (approx 2 GB) in release builds, or panics in debug builds.

**Fix:**
```rust
// Replace lines 107-118 with:
let n = contents.len();

// Bounds-check buf_len to prevent negative-to-usize wrapping (CR-N01)
let buf_len_usize = if buf_len < 0 {
    0
} else {
    buf_len as usize
};

if n <= buf_len_usize {
    let _ = memory.write(&mut caller, buf_ptr as usize, &contents);
    n as i32
} else {
    warn!(%session_id, "file_read: output buffer too small (need {}, have {})", n, buf_len_usize);
    let partial = &contents[..buf_len_usize];
    let _ = memory.write(&mut caller, buf_ptr as usize, partial);
    -1 // indicate truncation
}
```

---

## Info

### IN-01: `TenantQuotaLimiter::new_with_budget` duplicates `new` exactly (unfixed from original review)

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs:58-64`
**Issue:** The `new_with_budget` constructor is documented as "Lower-level constructor that accepts a pre-measured budget in bytes" but its implementation simply delegates to `Self::new(budget_max_mb, tenant_budget_used, inner)` -- identical behavior to the public `new` constructor, which also takes `budget_max_mb: u32`. The `#[doc(hidden)]` attribute hides it from docs but the function is dead weight with no distinct behavior. Was flagged in the original review as IN-01 but remains unfixed.
**Fix:** Either make `new_with_budget` accept `budget_max_bytes: usize` directly (to fulfill its documented purpose of accepting pre-measured bytes) or remove it and use `new` directly in tests.

### IN-02: Misleading comment in `normalize_path` test about parent traversal behavior (unfixed from original review)

**File:** `crates/jadepaw-wasm/src/path.rs:236-240`
**Issue:** The test comment on lines 236-240 contains scratch-work style commentary ("But actually: ..." / "Wait: ..." / "Let's trace: ...") that should be cleaned up. Was flagged in the original review as IN-02 but remains unfixed.
**Fix:** Replace lines 236-240 with a clean explanation:
```rust
    // "../../..": [] -> push ".." -> [".."] -> pop ".." -> []
    //             -> push ".." (stack empty) -> [".."]
```

---

## Verified Fixes

The following six fixes from the original review are confirmed correctly applied:

| Original ID | Description | Status |
|-------------|-------------|--------|
| CR-01 | Budget counter leak in `memory_growing` | Verified fixed. Delegate-before-commit pattern in `tenant_quota.rs:86-91`. |
| CR-02 | Budget counter leak in `table_growing` | Verified fixed. Same pattern in `tenant_quota.rs:107-110`. |
| WR-01 | i32 overflow in host function bounds checks | Verified fixed. All `(ptr + len) as usize` replaced with `checked_add` in `filesystem.rs`, `logging.rs`, `network.rs`. |
| WR-02 | Epoch ticker EngineWeak | Verified fixed. `epoch.rs:77` uses `engine_weak.upgrade()` for both liveness and increment; no `Engine` clone held in thread. |
| WR-03 | Path length cap | Verified fixed. `path.rs:43-46` adds `MAX_PATH_LEN = 4096` guard. |
| WR-04 | Stored `max_concurrent` for `capacity()` | Verified fixed. `pool.rs:133,173,249` stores and returns the configured value directly. |

| Original ID | Description | Status |
|-------------|-------------|--------|
| CR-03 | Missing `async_support(true)` | Intentionally skipped. `Config::async_support()` is deprecated and has no effect in wasmtime 45 (the version used by this project). Adding it would introduce a compiler warning. |

---

_Reviewed: 2026-05-30T19:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_