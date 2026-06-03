---
phase: 04-tool-system
plan: 02
subsystem: tooling
tags: [rust, reqwest, wasmtime, ssrf, file-io, http-client]

# Dependency graph
requires:
  - phase: 04-01
    provides: "Tool trait, ToolResult, ToolDefinition, HostFunctions::http_request"
  - phase: 02
    provides: "Wasm sandbox, file_read/write host fns, capability checks, validate_sandbox_path"
provides:
  - FileReadTool, FileWriteTool (Tool trait impls wrapping Wasm sandbox host functions)
  - HttpRequestTool (Tool trait impl with reqwest HTTP client + defense-in-depth SSRF)
  - http_request_host_fn (replaced Phase 2 stub with real HTTP execution + SSRF)
  - is_blocked_ip() pub(crate) utility for IP-layer SSRF blocking
  - extract_host_from_url() made pub(crate) for reuse across modules
affects: [04-03, agent-runtime]

# Tech tracking
tech-stack:
  added: [reqwest 0.12 (rustls-tls), async-trait 0.1, serde_json (new direct dep)]
  patterns: ["Tool trait implementations in tool_impls/ module", "Defense-in-depth SSRF (scheme + domain + IP layers)", "Wasm sandbox reuse via validate_sandbox_path in FileTool impls"]

key-files:
  created:
    - crates/jadepaw-wasm/src/tool_impls/mod.rs
    - crates/jadepaw-wasm/src/tool_impls/http_tool.rs
    - crates/jadepaw-wasm/src/tool_impls/file_tool.rs
  modified:
    - crates/jadepaw-wasm/Cargo.toml
    - crates/jadepaw-wasm/src/lib.rs
    - crates/jadepaw-wasm/src/host/network.rs
    - crates/jadepaw-wasm/src/host/mod.rs
    - Cargo.lock

key-decisions:
  - "Used reqwest 0.12 with rustls-tls (not native-tls) per project philosophy"
  - "is_blocked_ip() placed in host/network.rs as pub(crate) so both host fn and tool impl can use it"
  - "FileReadTool/FileWriteTool reuse validate_sandbox_path from Phase 2 — no duplicate sandbox logic"
  - "No HostFunctions impl in jadepaw-wasm exists yet — trait is defined in jadepaw-core with only test impl"
  - "Response body capped at 1MB in both host fn and tool impl (defense-in-depth)"

patterns-established:
  - "Tool impl → validate_sandbox_path → I/O: file tools delegate to Phase 2 sandbox boundary"
  - "Defense-in-depth SSRF: scheme validation → domain whitelist → IP-layer check → redirect limit → body cap"
  - "Host function returns i32 (-1 on error, status_code on success) per D-04b contract"

requirements-completed: [AGENT-02]

# Metrics
duration: 9m31s
completed: 2026-06-03
---

# Phase 04 Plan 02: Tool Implementations Summary

**File I/O (read/write) and HTTP request tools with reqwest-based SSRF protection, wrapping the Wasm sandbox and real HTTP client with defense-in-depth security**

## Performance

- **Duration:** 9m31s
- **Started:** 2026-06-03 (Plan execution start)
- **Completed:** 2026-06-03
- **Tasks:** 3
- **Files modified:** 8 (4 created, 4 modified)

## Accomplishments

- `HttpRequestTool` implementing `Tool` trait with real `reqwest` HTTP client, SSRF IP-layer blocking, scheme validation, redirect limit (1), 30s timeout, and 1MB body cap
- `FileReadTool` and `FileWriteTool` implementing `Tool` trait, reusing Phase 2's `validate_sandbox_path` for path containment with no duplicate sandbox logic
- `http_request_host_fn` stub replaced with real HTTP execution: DNS resolution with 5s timeout, IP-layer SSRF check, method validation, reqwest-based HTTP with security defaults
- `is_blocked_ip()` pub(crate) utility shared between host function and tool impl for defense-in-depth SSRF
- All 76 existing Phase 2 tests pass, all three tool impls compile cleanly with zero clippy warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Add reqwest dependency + implement HttpRequestTool with SSRF** - `451007b` (feat)
2. **Task 2: Implement FileReadTool + FileWriteTool wrapping Wasm host functions** - `f4b1f77` (feat)
3. **Task 3: Fill in http_request_host_fn stub + update HostFunctions impl + wire lib.rs** - `b3c6706` (feat)

## Files Created/Modified

**Created:**
- `crates/jadepaw-wasm/src/tool_impls/mod.rs` — Module declaration and re-exports for FileReadTool, FileWriteTool, HttpRequestTool
- `crates/jadepaw-wasm/src/tool_impls/http_tool.rs` — HttpRequestTool with reqwest, SSRF protection, 1MB body cap, 30s timeout, redirect::Policy::limited(1)
- `crates/jadepaw-wasm/src/tool_impls/file_tool.rs` — FileReadTool and FileWriteTool wrapping validate_sandbox_path with structured ToolResult errors

**Modified:**
- `crates/jadepaw-wasm/Cargo.toml` — Added reqwest 0.12 (rustls-tls), async-trait 0.1, serde_json direct dep
- `crates/jadepaw-wasm/src/lib.rs` — Added `pub mod tool_impls` and re-exports of all three tool types
- `crates/jadepaw-wasm/src/host/network.rs` — Replaced Phase 2 HTTP stub with real reqwest execution + SSRF; made extract_host_from_url pub(crate); added is_blocked_ip pub(crate)
- `Cargo.lock` — Updated with reqwest and webpki-roots

## Decisions Made

- **reqwest version:** Used 0.12 (minimum compatible) since Cargo.lock had 0.13.4. The plan specified 0.12 as the stable line used by async-openai. `cargo` resolved to 0.12.28.
- **is_blocked_ip location:** Placed in `host/network.rs` as `pub(crate)` so both `http_request_host_fn` and `HttpRequestTool::call()` can import it via `crate::host::network::is_blocked_ip()`.
- **No HostFunctions impl update needed:** The only `impl HostFunctions` is `TestHostFn` in `jadepaw-core/tests/`, which already has `http_request`. No production `HostFunctions` impl exists in `jadepaw-wasm`.
- **File tools use validate_sandbox_path directly:** Per D-01a, the Tool impl wraps the sandbox boundary rather than calling through the Wasm host function (which requires a running Store/Caller).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added missing async-trait dependency**
- **Found during:** Task 3 (full build)
- **Issue:** Both file_tool.rs and http_tool.rs use `#[async_trait]` but `async-trait` was not in Cargo.toml dependencies
- **Fix:** Added `async-trait = "0.1"` to jadepaw-wasm/Cargo.toml
- **Files modified:** crates/jadepaw-wasm/Cargo.toml

**2. [Rule 1 - Bug] Fixed unused imports and dead code warnings**
- **Found during:** Task 3 compilation
- **Issue:** Unused `JadepawError` import in file_tool.rs, unused `IpAddr/Ipv4Addr/Ipv6Addr` in http_tool.rs, dead_code warnings on session_id fields
- **Fix:** Removed unused imports, added `#[allow(dead_code)]` on FileReadTool/FileWriteTool structs (session_id stored for future logging)
- **Files modified:** crates/jadepaw-wasm/src/tool_impls/file_tool.rs, crates/jadepaw-wasm/src/tool_impls/http_tool.rs

**3. [Rule 1 - Bug] Fixed tokio::net::lookup_host return type mismatch**
- **Found during:** Task 3 compilation
- **Issue:** `lookup_host` returns `impl Iterator<Item = SocketAddr>`, not `Vec<SocketAddr>`. Original code tried to call `.collect()` on `Vec<SocketAddr>` which doesn't have `.collect()`.
- **Fix:** Restructured DNS resolution to handle the nested Result from timeout + lookup correctly: `Ok(Ok(iter))` pattern with `.collect()` on the iterator
- **Files modified:** crates/jadepaw-wasm/src/host/network.rs

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All auto-fixes necessary for compilation and correctness. No scope creep.

## Issues Encountered

- **Pre-existing clippy warnings in jadepaw-core:** `cargo clippy -p jadepaw-wasm -- -D warnings` fails due to `derivable_impls` warnings in `jadepaw-core` (agent_types.rs, guest_exports.rs), which are pre-existing from Plan 01. The `jadepaw-wasm` crate itself has zero clippy warnings.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- All three Tool trait impls compile and pass existing Phase 2 tests (76 tests, 0 failures)
- HttpRequestTool, FileReadTool, and FileWriteTool are ready for registration in ToolRegistry (Plan 04-03)
- http_request_host_fn is no longer a stub — ready for real guest-host HTTP calls

---
*Phase: 04-tool-system*
*Completed: 2026-06-03*