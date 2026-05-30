---
phase: 02-wasm-isolation-core
reviewed: 2026-05-30T23:00:00Z
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
  critical: 1
  warning: 1
  info: 1
  total: 3
status: issues_found
---

# Phase 02: Code Review Report (Round 4 -- Regression Pass)

**Reviewed:** 2026-05-30T23:00:00Z
**Depth:** standard
**Files Reviewed:** 32
**Status:** issues_found

## Summary

Fourth adversarial review pass on the wasm-isolation-core phase (32 source files). This pass focuses on **regression checking** — verifying all prior fixes (Rounds 1-3) remain intact, and identifying any remaining defects missed in earlier rounds.

### Prior Round Fix Verification

All fixes from Rounds 1-3 verified as still correct:

| ID | Description | Status |
|----|-------------|--------|
| CR-01/02 | Budget counter leak (delegate-before-commit) | **Still correct.** `tenant_quota.rs:86-91,107-110`. |
| WR-01 | i32 overflow via `checked_add` | **Still correct.** All four host functions use `checked_add` consistently. |
| WR-02 | Epoch `EngineWeak` liveness detection | **Still correct.** `epoch.rs:77-83`. |
| WR-03 | Path length cap at 4096 | **Still correct.** `path.rs:44-47`. |
| WR-04 | Stored `max_concurrent` for `capacity()` | **Still correct.** `pool.rs:133,247-249`. |
| CR-N01 | Negative `buf_len` sanitization | **Still correct.** `filesystem.rs:108`. |
| WR-N01 (R3) | Domain bare `"*"` wildcard | **Still correct.** `capability/mod.rs:101-103` + test at line 169-174. |
| WR-N02 (R3) | Partial write on buffer-too-small | **Still correct.** `filesystem.rs:112-119` — returns `-1` without writing data. |
| CR-03 | Missing `async_support(true)` | **Intentionally skipped.** Deprecated no-op in wasmtime 45. Confirmed no effect. |

### Compilation Status

`cargo check --workspace`: clean. All 44 tests pass (9 in jadepaw-core, 35 in jadepaw-wasm including 1 `#[ignore]` stress test). Clippy reports 3 `new_without_default` warnings on ID types (non-blocking, INFO).

### New Findings

One BLOCKER, one WARNING, one INFO. Details below.

---

## Critical Issues

### CR-01: `file_read_host_fn` silently ignores `memory.write` failure, returning false-positive byte count

**File:** `crates/jadepaw-wasm/src/host/filesystem.rs:112-115`
**Issue:** When the guest buffer (`buf_ptr` + `buf_len`) is declared large enough for the file contents, the function calls `memory.write(&mut caller, buf_ptr as usize, &contents)` on line 114 and discards the `Result` with `let _`. However, `memory.write` can fail even when `n <= buf_len_usize` — specifically when `buf_ptr + contents.len()` exceeds the actual guest memory size (`Memory::data_size`). `buf_ptr` is completely untrusted guest input and is never bounds-checked.

When this `memory.write` call returns `Err(MemoryAccessError)`, the error is silently discarded and the function returns `n` (a positive byte count). The guest receives a **false-positive** return value — it believes the file was read successfully and data was written to its buffer, when in fact no data was written. The guest's buffer contains whatever was there before (potentially stale data from a previous operation in that pooled memory slot, or uninitialized zeros).

This is a correctness bug: the guest can act on stale/garbage data as if it were the actual file contents. While not a sandbox escape (the write is contained within guest memory), it violates the contract of the `file_read` host function.

**Reproduction scenario:**
1. Guest sets `buf_len = 1024` (large enough for file contents) but `buf_ptr = memory_size - 10` (near memory boundary)
2. File is 20 bytes long. `n (20) <= buf_len_usize (1024)` is true.
3. `memory.write(&mut caller, buf_ptr, &contents)` returns `Err(MemoryAccessError)` because `buf_ptr + 20 > memory_size`
4. The `let _` discards the error. Function returns `20` (success).
5. Guest reads its buffer at offset `buf_ptr` — finds stale data, believes it's the file contents.

**Fix:** Check the `memory.write` return value and return `-1` on failure:

```rust
let n = contents.len() as i32;
if n as usize <= buf_len_usize {
    // buf_ptr is guest-controlled and untrusted — memory.write can still fail
    // if buf_ptr + contents.len() exceeds actual memory bounds
    match memory.write(&mut caller, buf_ptr as usize, &contents) {
        Ok(()) => n,
        Err(e) => {
            warn!(%session_id, "file_read: memory.write failed (buf_ptr={}, len={}): {}", buf_ptr, n, e);
            -1
        }
    }
} else {
    warn!(%session_id, "file_read: output buffer too small (need {}, have {})", n, buf_len);
    -1
}
```

---

## Warnings

### WR-01: `path_matches` prefix matching for `"/*"` suffix is overly broad — matches paths sharing the prefix as a substring

**File:** `crates/jadepaw-wasm/src/capability/mod.rs:81-82`
**Issue:** When a `PathPattern` uses the `"/*"` suffix, `strip_suffix("/*")` extracts the prefix, then `path.starts_with(prefix)` checks if the path starts with that prefix. This is overly broad: pattern `"data/*"` produces prefix `"data"`, and `"data_extra/secret.txt".starts_with("data")` returns `true`. The guest would unexpectedly gain read access to files in `data_extra/` when only `data/` was intended.

Similarly for bare-`*` suffix: pattern `"data*"` produces prefix `"data"`, and `"data_extra.txt".starts_with("data")` matches. The `"/*"` variant specifically implies a directory boundary that the current code does not enforce.

**Note:** The `"/*"` suffix test at line 143 (`path_matches_prefix`) uses `"data/nested/file.txt"` which correctly matches, but the edge case `assert!(!path_matches("data_extra/file.txt", "data/*"))` is missing from the test suite.

**Fix:** For the `"/*"` variant, ensure the next character after the prefix is `/`:

```rust
if let Some(prefix) = pattern.strip_suffix("/*") {
    // Must either be exactly the prefix (empty path after prefix) or
    // the prefix followed by '/' to enforce directory boundary
    return path == prefix
        || (path.starts_with(prefix) && path.as_bytes().get(prefix.len()) == Some(&b'/'));
}
```

For the bare-`*` variant, the current behavior (`starts_with`) is arguably correct since there's no directory boundary implied. Consider documenting this difference or adding a `min_len` check to prevent exact prefix match if desired.

---

## Info

### IN-01: Missing `Default` impl on `SessionId`, `TenantId`, `ToolId` (clippy `new_without_default`)

**File:** `crates/jadepaw-core/src/types.rs:18,45,72`
**Issue:** All three newtype wrappers implement `fn new() -> Self` without a corresponding `Default` impl. Clippy reports `new_without_default` warnings on all three. While `new()` creates a new UUID v7, `Default` should delegate to `new()` to follow Rust conventions.

**Fix:** Add `Default` impls:

```rust
impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}
impl Default for TenantId {
    fn default() -> Self {
        Self::new()
    }
}
impl Default for ToolId {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Verified Fixes (Rounds 1-3)

All eight applied fixes from prior rounds remain correctly in place and pass tests:

| Original ID | Description | Status |
|-------------|-------------|--------|
| CR-01 | Budget counter leak in `memory_growing` | Verified. `tenant_quota.rs:86-91` delegate-before-commit. |
| CR-02 | Budget counter leak in `table_growing` | Verified. `tenant_quota.rs:107-110` same pattern. |
| WR-01 | i32 overflow in host function bounds checks | Verified. All ptr/len pairs use `checked_add` in all four host functions. |
| WR-02 | Epoch ticker EngineWeak | Verified. `epoch.rs:77-83`. |
| WR-03 | Path length cap at 4096 | Verified. `path.rs:44-47`. |
| WR-04 | Stored `max_concurrent` for `capacity()` | Verified. `pool.rs:133,247-249`. |
| CR-N01 | Negative `buf_len` sanitization | Verified. `filesystem.rs:108`. |
| WR-N01 (R3) | Domain bare `"*"` wildcard | Verified. `capability/mod.rs:101-103` + test at lines 169-174. |
| WR-N02 (R3) | Partial write on buffer-too-small | Verified. `filesystem.rs:112-119` clean fail, no partial write. |
| CR-03 | Missing `async_support(true)` | Intentionally skipped. Wasmtime 45 makes `async_support` a deprecated no-op. |

---

_Reviewed: 2026-05-30T23:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_