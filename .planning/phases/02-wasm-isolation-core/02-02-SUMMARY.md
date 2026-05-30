---
phase: 02-wasm-isolation-core
plan: 02
subsystem: wasm-runtime
tags: [capability-enforcement, path-validation, host-functions, security]
requires: ["02-01"]
provides: ["Host function mediation via capability whitelist", "Path traversal prevention", "Default-deny enforcement"]
affects: []
tech-stack:
  added: []
  patterns:
    - "Linker<SessionState> with func_wrap_async under jadepaw namespace"
    - "Capability check methods on SessionState (capability module)"
    - "Path validation: normalize_path + validate_sandbox_path with canonicalize+prefix"
    - "Host functions: guard (capability + path) before any I/O side effects"
key-files:
  created:
    - "crates/jadepaw-wasm/src/path.rs — normalize_path/validate_sandbox_path"
    - "crates/jadepaw-wasm/src/capability/mod.rs — can_read_file/write_file/call_tool/access_domain"
    - "crates/jadepaw-wasm/src/host/mod.rs — host function re-exports"
    - "crates/jadepaw-wasm/src/host/logging.rs — log_message host fn"
    - "crates/jadepaw-wasm/src/host/filesystem.rs — file_read/file_write host fns"
    - "crates/jadepaw-wasm/src/host/network.rs — http_request stub"
    - "crates/jadepaw-wasm/src/linker.rs — create_linker + register_host_functions"
    - "crates/jadepaw-wasm/tests/path_validation.rs — 24 unit tests"
    - "crates/jadepaw-wasm/tests/capability.rs — 9 integration tests"
    - "crates/jadepaw-wasm/tests/fixtures/tool_caller.wat — Wasm guest module"
  modified:
    - "crates/jadepaw-wasm/src/session.rs — added sandbox_root field"
    - "crates/jadepaw-wasm/src/lib.rs — new module declarations and re-exports"
    - "crates/jadepaw-wasm/Cargo.toml — added tempfile dev-dependency"
    - "crates/jadepaw-wasm/tests/engine_smoke.rs — updated SessionState::new calls"
    - "crates/jadepaw-wasm/tests/limits.rs — updated SessionState::new calls"
decisions:
  - "D-11: can_* methods on SessionState in dedicated capability module"
  - "D-12: Default deny enforcement — empty capability lists deny all operations"
  - "D-02: Host functions registered under jadepaw namespace via func_wrap_async"
  - "SEC-03: Path validation with normalize + canonicalize + sandbox prefix check"
  - "SEC-04: Capability whitelist default deny verified via 9 integration tests"
  - "Rule 1 bug fix: normalize_path preserves .. when stack is empty for traversal detection"
  - "Rule 1 bug fix: validate_sandbox_path handles nonexistent paths (file_write target)"
metrics:
  duration: "~15min"
  completed_date: "2026-05-30"
---

# Phase 02 Plan 02: Host Mediation, Capability Enforcement, and Path Validation Summary

**One-liner:** Implemented host function mediation with capability whitelist and path sandbox boundary enforcement — every guest→host call is gated by `can_*` checks on `SessionState` before any I/O side effects.

## Results

64 tests pass: 19 lib unit + 9 capability integration + 3 engine smoke + 8 resource limits + 24 path validation + 1 doctest.

### What was built

1. **Path validation** (`path.rs`) — `normalize_path` removes `.` and resolves `..` components; `validate_sandbox_path` joins with sandbox root, resolves via canonicalize (or parent canonicalize for new files), and verifies containment with `starts_with` prefix check. Prevents path traversal attacks (SEC-03).

2. **Capability enforcement** (`capability/mod.rs`) — `SessionState::can_read_file`, `can_write_file`, `can_call_tool`, `can_access_domain` methods. Pattern matching supports exact match, prefix match (`data/*`), and wildcard (`*`). Default deny: empty Vecs return false for all checks (D-12).

3. **Host functions** — Four host functions registered on `Linker<SessionState>` under the `"jadepaw"` namespace (D-02) via `func_wrap_async`:
   - `log_message` — always allowed, safe default, session_id always logged
   - `file_read` — capability check + path validation → I/O
   - `file_write` — capability check + path validation → I/O
   - `http_request` — domain capability check, stub in Phase 2 (returns CapabilityDenied)

4. **SessionState sandbox_root** — Added `sandbox_root: PathBuf` field for path containment. All filesystem host functions use this to validate paths.

5. **Integration tests** — `tool_caller.wat` guest module exercises all host function imports. 9 end-to-end tests verify: log_message always succeeds, file_read with/without capability, path traversal rejection on both read and write, file_write with capability, default deny, and session_id access in host functions.

### Verified success criteria

- [x] `validate_sandbox_path` rejects path traversal (`"../../../etc/passwd"`) → PathValidationError
- [x] `can_read_file` returns false for path not in whitelist (default deny per D-12)
- [x] Host functions registered under `"jadepaw"` namespace via `func_wrap_async` (D-02)
- [x] Every host function accesses `caller.data()` at entry before side effects (D-11)
- [x] Default deny: empty capability whitelist denies all operations (D-12)
- [x] Path traversal rejected before I/O (SEC-03)
- [x] `file_read` with capability grant succeeds within sandbox
- [x] `file_read` without capability grant returns -1 (CapabilityDenied, SEC-04)
- [x] `log_message` always succeeds (safe default, session_id logged)
- [x] Guest memory (ptr, len) bounds-checked in all host functions (Pitfall 3)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] validate_sandbox_path failed on nonexistent file paths**
- **Found during:** Task 2, test_file_write_allowed
- **Issue:** `validate_sandbox_path` called `Path::canonicalize()` on the candidate path, which requires the file to exist. This blocked file_write operations on new files.
- **Fix:** When the candidate path does not exist, canonicalize the parent directory instead and re-join with the filename. The parent prefix check prevents traversal. This is safe because the filename is a normalized leaf component with no `..` or `.` possible.
- **Files modified:** `crates/jadepaw-wasm/src/path.rs`
- **Commit:** `5f4a114`

**2. [Rule 1 - Bug] normalize_path dropped `..` components when stack was empty**
- **Found during:** Task 2, test_path_traversal_file_write
- **Issue:** When the component stack was empty, `..` was silently dropped, causing `"../outside"` to normalize to `"outside"` — a valid sandbox path. This escaped sandbox traversal detection.
- **Fix:** When the stack is empty and a `..` is encountered, push it to the output (`components.push("..")`). This preserves `..` in the normalized form so `validate_sandbox_path` can catch the traversal via canonicalize+prefix.
- **Files modified:** `crates/jadepaw-wasm/src/path.rs`
- **Commit:** `5f4a114`

**3. [Rule 2 - Missing Critical Functionality] SessionState::with_defaults convenience constructor**
- **Found during:** Task 1 implementation
- **Issue:** Testing code needed a simple way to create SessionState with default capabilities and a given sandbox root.
- **Fix:** Added `SessionState::with_defaults(sandbox_root: PathBuf)` constructor.
- **Files modified:** `crates/jadepaw-wasm/src/session.rs`
- **Commit:** `5f4a114`

**4. [Rule 3 - Blocking Issue] func_wrap_async parameter mismatch**
- **Found during:** Task 1 compilation
- **Issue:** `func_wrap_async` expects a closure taking `(Caller<'a, T>, Params)` where `Params` is a tuple type. The host functions were written as standalone functions taking individual parameters. This caused a "expected 2 args but takes 5" compile error.
- **Fix:** Wrapped each host function in a closure in `linker.rs` that destructures the tuple parameter.
- **Files modified:** `crates/jadepaw-wasm/src/linker.rs`
- **Commit:** `5f4a114`

**5. [Rule 3 - Blocking Issue] wasmtime::Error doesn't implement std::error::Error**
- **Found during:** Task 1 compilation
- **Issue:** `func_wrap_async` returns `Result<_, wasmtime::Error>`, and `anyhow::Error: From<E>` requires `E: std::error::Error`. wasmtime 45.0's Error type doesn't implement this trait, so `?` operator failed.
- **Fix:** Used `.map_err(|e| anyhow::anyhow!("...: {e}"))` to manually convert each `wasmtime::Error` to `anyhow::Error`.
- **Files modified:** `crates/jadepaw-wasm/src/linker.rs`
- **Commit:** `5f4a114`

## Commits

1. `8016b04` — `test(02-02): add failing tests for path validation and capability checks (RED)` — 24 unit tests
2. `5f4a114` — `feat(02-02): implement path validation, capability checks, and host functions (GREEN)` — all implementation code
3. `d9c2fa1` — `feat(02-02): add integration tests for capability enforcement and host mediation` — 9 integration tests + WAT fixture

## Known Stubs

| File | Line | Description |
|------|------|-------------|
| `crates/jadepaw-wasm/src/host/network.rs` | `http_request_host_fn` | Returns -1 (CapabilityDenied) for all requests. Full HTTP implementation deferred to Phase 4. Domain validation and bounds-checking are active. |

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: toctou-window | `crates/jadepaw-wasm/src/path.rs` | `validate_sandbox_path` checks `candidate.exists()` then either canonicalizes directly or via parent. There is a TOCTOU window between the `exists()` check and `canonicalize()`/file I/O where a symlink could be created. Mitigation: parent canonicalization limits the window to the filename component only. Full `openat2(RESOLVE_NO_SYMLINKS)` or equivalent should be used on Linux in production (Phase 4 hardening). |

## Self-Check: PASSED

- `crates/jadepaw-wasm/src/path.rs` — FOUND
- `crates/jadepaw-wasm/src/capability/mod.rs` — FOUND
- `crates/jadepaw-wasm/src/host/mod.rs` — FOUND
- `crates/jadepaw-wasm/src/host/logging.rs` — FOUND
- `crates/jadepaw-wasm/src/host/filesystem.rs` — FOUND
- `crates/jadepaw-wasm/src/host/network.rs` — FOUND
- `crates/jadepaw-wasm/src/linker.rs` — FOUND
- `crates/jadepaw-wasm/tests/path_validation.rs` — FOUND
- `crates/jadepaw-wasm/tests/capability.rs` — FOUND
- `crates/jadepaw-wasm/tests/fixtures/tool_caller.wat` — FOUND
- `8016b04` — FOUND in git log
- `5f4a114` — FOUND in git log
- `d9c2fa1` — FOUND in git log