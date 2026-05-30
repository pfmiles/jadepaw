---
phase: 02-wasm-isolation-core
reviewed: 2026-05-30T22:00:00Z
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
  critical: 0
  warning: 2
  info: 0
  total: 2
status: issues_found
---

# Phase 02: Code Review Report (Round 3 -- Post Round 1+2 Fixes)

**Reviewed:** 2026-05-30T22:00:00Z
**Depth:** standard
**Files Reviewed:** 32
**Status:** issues_found

## Summary

Third adversarial review pass on the wasm-isolation-core phase (32 source files), following two prior rounds of fixes:

- **Round 1**: CR-01, CR-02 (budget counter leak in `TenantQuotaLimiter`), WR-01 (i32 overflow in bounds checks via `checked_add`), WR-02 (epoch `EngineWeak` for liveness detection), WR-03 (path length cap at 4096), WR-04 (stored `max_concurrent` for reliable `capacity()`)
- **Round 2**: CR-N01 (`buf_len` sanitization in `file_read_host_fn` to prevent negative-to-usize wrap panic)
- **CR-03** intentionally deferred (wasmtime 45 deprecated `Config::async_support()`)

### Verified Fixes (All Correct)

All six prior fixes hold solid:

- **CR-01/CR-02**: Delegate-before-commit pattern in `tenant_quota.rs` correctly prevents budget counter leak when inner `InstanceHardLimiter` rejects growth
- **WR-01**: `checked_add` is consistently applied across all four host functions (`filesystem.rs`, `logging.rs`, `network.rs`) for every pointer+length pair
- **WR-02**: `epoch.rs` `engine_weak.upgrade()` releases the `Engine` reference immediately after `increment_epoch()`, and exits when `upgrade()` returns `None`
- **WR-03**: `normalize_path` returns sentinel `PathBuf::from("..")` for paths exceeding 4096 bytes, which `validate_sandbox_path` correctly rejects
- **WR-04**: `InstancePool::capacity()` returns the stored `max_concurrent` field directly, decoupled from `Semaphore::available_permits()` best-effort API
- **CR-N01**: `file_read_host_fn` line 108 sanitizes `buf_len` via `if buf_len > 0 { buf_len as usize } else { 0 }`, preventing negative-to-`usize::MAX` wrap that caused host panic in Round 2

### Test Results

All 44 tests pass (9 in jadepaw-core, 35 in jadepaw-wasm including 1 `#[ignore]` stress test). The code compiles cleanly with no warnings.

### Overall Assessment

The core isolation architecture is sound. Resource limiters follow a correct delegating chain, capability checks enforce default-deny, path validation prevents sandbox escape, and memory safety is maintained via consistent `checked_add` usage. Two new findings are flagged below, both WARNING severity with simple fixes. No critical/blocker issues remain.

---

## Warnings

### WR-01: `domain_matches` lacks bare `"*"` wildcard support (inconsistency with `path_matches`)

**File:** `crates/jadepaw-wasm/src/capability/mod.rs:99-114`
**Issue:** `path_matches` (line 74-90) supports three patterns: exact match, `"prefix/*"` prefix match, `"prefix*"` bare-suffix prefix match, and bare `"*"` (match-everything). `domain_matches` (line 99-114) only supports exact match and `"*.suffix.com"` subdomain wildcard. There is no equivalent of bare `"*"` for domain patterns.

This asymmetry means `DomainPattern("*")` -- a natural expression of "allow all domains" analogous to `PathPattern("*")` -- silently falls through to `false` (default deny). While this is safe (default deny), it is an inconsistency in the public API that could surprise consumers when network functionality goes live in Phase 4.

**Fix:**
```rust
fn domain_matches(domain: &str, pattern: &str) -> bool {
    // Wildcard matches everything (consistent with path_matches)
    if pattern == "*" {
        return true;
    }

    // Exact match
    if domain == pattern {
        return true;
    }

    // Wildcard subdomain: "*.example.com" matches "api.example.com"
    if let Some(suffix) = pattern.strip_prefix("*.") {
        return domain.ends_with(suffix)
            && domain.len() > suffix.len()
            && domain.as_bytes()[domain.len() - suffix.len() - 1] == b'.';
    }

    false
}
```

---

### WR-02: `file_read_host_fn` truncation path writes partial data but returns `-1` (ambiguous failure mode)

**File:** `crates/jadepaw-wasm/src/host/filesystem.rs:111-121`
**Issue:** When the guest-provided buffer is too small for the file contents, the function:
1. Writes partial (truncated) data into guest memory (line 120)
2. Returns `-1` (error indicator)

This creates an ambiguous state: guest memory contains data from a partial read, but the return code says "error." The guest cannot distinguish "partial read due to small buffer" from "genuine I/O failure." A guest that interprets `-1` as "retry" would re-read, lose the partial data already written, and possibly loop.

**Fix:** Write nothing on truncation (clean fail), which is simplest and safest for MVP:
```rust
let n = contents.len() as i32;
if n as usize <= buf_len_usize {
    let _ = memory.write(&mut caller, buf_ptr as usize, &contents);
    n
} else {
    warn!(%session_id, "file_read: output buffer too small (need {}, have {})", n, buf_len);
    // Do NOT write partial data -- return clean error with guest memory unchanged
    -1
}
```

---

## Verified Fixes (Round 1 + Round 2)

All six applied fixes remain correctly in place:

| Original ID | Description | Status |
|-------------|-------------|--------|
| CR-01 | Budget counter leak in `memory_growing` | **Verified.** Delegate-before-commit at `tenant_quota.rs:86-91`. |
| CR-02 | Budget counter leak in `table_growing` | **Verified.** Same pattern at `tenant_quota.rs:107-110`. |
| WR-01 | i32 overflow in host function bounds checks | **Verified.** All ptr/len pairs use `checked_add` in `filesystem.rs`, `logging.rs`, `network.rs`. |
| WR-02 | Epoch ticker EngineWeak | **Verified.** `epoch.rs:77-83` upgrades weak ref, increments epoch, drops ref; exits on `None`. |
| WR-03 | Path length cap | **Verified.** `path.rs:44-47` guards via `MAX_PATH_LEN = 4096`. |
| WR-04 | Stored `max_concurrent` for `capacity()` | **Verified.** `pool.rs:133,173,247-249` stores and returns configured capacity directly. |
| CR-N01 | Negative `buf_len` sanitization | **Verified.** `filesystem.rs:108` uses `if buf_len > 0 { buf_len as usize } else { 0 }`. |
| CR-03 | Missing `async_support(true)` | **Intentionally skipped.** Deprecated no-op in wasmtime 45. |

---

_Reviewed: 2026-05-30T22:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_