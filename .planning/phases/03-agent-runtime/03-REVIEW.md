---
phase: 03-agent-runtime
reviewed: 2026-06-02T08:00:00Z
depth: standard
files_reviewed: 49
files_reviewed_list:
  - Cargo.lock
  - Cargo.toml
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/guard.rs
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/stream.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-agent/tests/sse_streaming.rs
  - crates/jadepaw-agent/tests/termination.rs
  - crates/jadepaw-core/Cargo.toml
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/capabilities.rs
  - crates/jadepaw-core/src/error.rs
  - crates/jadepaw-core/src/guest_exports.rs
  - crates/jadepaw-core/src/host_functions.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/types.rs
  - crates/jadepaw-core/tests/agent_types.rs
  - crates/jadepaw-core/tests/capabilities.rs
  - crates/jadepaw-core/tests/host_functions.rs
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
  - crates/jadepaw-wasm/tests/epoch_yield.rs
  - crates/jadepaw-wasm/tests/fixtures/noop.wat
  - crates/jadepaw-wasm/tests/fixtures/tool_caller.wat
  - crates/jadepaw-wasm/tests/limits.rs
  - crates/jadepaw-wasm/tests/path_validation.rs
  - crates/jadepaw-wasm/tests/pool.rs
  - crates/jadepaw-wasm/tests/stress_concurrent.rs
  - docs/architecture.md
findings:
  critical: 0
  warning: 4
  info: 5
  total: 9
status: issues_found
---

# Phase 03: Code Review Report

**Reviewed:** 2026-06-02T08:00:00Z
**Depth:** standard
**Files Reviewed:** 49
**Status:** issues_found

## Summary

Reviewed the full Phase 03 agent runtime implementation: `jadepaw-agent` (ReAct loop, LLM integration, SSE streaming, termination guards), `jadepaw-core` (shared types, error types, guest exports, capabilities), and `jadepaw-wasm` (engine, pool, session, host functions, resource limits, path validation). The code compiles cleanly with zero warnings.

The previous review's critical findings (CR-01: indiscriminate WasmTrap catch-all and CR-02: sub-second timeout truncation) have been properly addressed — the code now uses `InfrastructureError` for LLM/channel errors and `WallClockTimeout` with millisecond-precision fields. The structural findings (WR-01: missing thought in trace, WR-02: unscoped directive search, WR-03: dead test) have also been fixed.

No new critical issues were found. Four warnings cover: `TenantQuotaLimiter` defined but never wired into the pool infrastructure, unused dependencies in `jadepaw-agent/Cargo.toml`, `GuardConfig` taken by value preventing reuse across sessions, and the `new_with_budget` constructor having a misleading doc comment. Five informational items cover: outdated `async_support` doc comment, `context` parameter only applied to first turn, `log_message` missing debug/trace level routing, `domain_matches` limitations for internal wildcards, and `normalize_path` using a sentinel value for over-long paths.

---

## Warnings

### WR-01: `TenantQuotaLimiter` defined and exported but never wired into pool/session infrastructure

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs` (entire module), `crates/jadepaw-wasm/src/session.rs:24-27`
**Issue:** `TenantQuotaLimiter` is a fully-implemented `ResourceLimiter` with correct delegation semantics, but it is never instantiated in production code. `SessionLimits` only contains an `InstanceHardLimiter` (`session.rs:26`). The `store.limiter()` closure in `pool.rs:214` only provides the hard limiter: `store.limiter(|s| &mut s.limits.hard_limit)`. No code path wraps `InstanceHardLimiter` in a `TenantQuotaLimiter`. This means tenant-level aggregate budget enforcement is completely absent — every instance operates independently with only the per-instance 64MB cap.

**Fix:** Either wire `TenantQuotaLimiter` into `SessionLimits` with a default-unlimited budget, or explicitly document that this is deferred to a later phase. For an immediate fix, the `SessionLimits` struct could store an `Option<TenantQuotaLimiter>`:

```rust
// session.rs
pub struct SessionLimits {
    pub hard_limit: InstanceHardLimiter,
    pub tenant_quota: Option<TenantQuotaLimiter>,
}
```

Alternatively, if tenant quotas are truly deferred, add a doc comment on the module:
```rust
//! Note: TenantQuotaLimiter is implemented and tested but not yet wired
//! into the InstancePool/SessionState infrastructure. It will be activated
//! when per-tenant aggregate memory tracking is needed (Phase 4).
```

### WR-02: Unused dependencies in `jadepaw-agent/Cargo.toml`

**File:** `crates/jadepaw-agent/Cargo.toml:10,19,29-33`
**Issue:** Three declared dependencies have no corresponding usage in any source file under `crates/jadepaw-agent/src/`:
- `jadepaw-bus` (line 10) — planned for Phase 3+ but not yet imported
- `async-trait` (line 19) — no `#[async_trait]` usage anywhere in the agent crate
- `redis` (lines 29-33, optional) — no redis code paths exist in the agent crate

Declaring dependencies before they are used adds unnecessary compilation overhead, increases attack surface for `cargo-audit`, and makes dependency auditing harder. The `redis` feature is especially problematic since it defines features (`single-node`, `cluster`, `redis`) that have no behavioral effect.

**Fix:** Remove `jadepaw-bus`, `async-trait`, and the redis feature block from `jadepaw-agent/Cargo.toml`. Add them back when the implementing code is committed:
```diff
- jadepaw-bus = { path = "../jadepaw-bus" }
- async-trait = "0.1"
- 
- [features]
- default = ["single-node"]
- single-node = []
- cluster = ["redis"]
- redis = ["dep:redis"]
- 
- [dependencies.redis]
- workspace = true
- optional = true
```

### WR-03: `GuardConfig` consumed by value in `run_with_guard`, preventing reuse

**File:** `crates/jadepaw-agent/src/guard.rs:47-48`, `crates/jadepaw-agent/src/lib.rs:91,95`
**Issue:** `run_with_guard` takes `config: GuardConfig` by value, which moves ownership. In `run_agent` (lib.rs:91,95), `guard_config` is created inline and passed directly, so it works for a single call. However, for a production server processing thousands of concurrent requests, the same `GuardConfig` would ideally be shared across all sessions. Taking by value forces unnecessary cloning or per-call construction.

**Fix:** Change to borrow, matching the pattern used by `LoopConfig` (also constructed inline but could benefit from sharing):
```rust
// guard.rs:47
pub async fn run_with_guard<F, Fut>(
    config: &GuardConfig,  // borrowed instead of owned
    agent_loop: F,
) -> Result<Vec<ReActStep>, JadepawError>

// guard.rs:55
tokio::select! {
    result = agent_loop() => { ... }
    _ = tokio::time::sleep(config.wall_clock_timeout) => { ... }
}
```

Note: `tokio::time::sleep` takes `Duration` by value (Copy), so borrowing `config` still works correctly for the timeout branch.

### WR-04: `TenantQuotaLimiter::new_with_budget` doc comment is misleading

**File:** `crates/jadepaw-wasm/src/limits/tenant_quota.rs:55-63`
**Issue:** The `new_with_budget` method's doc comment says "Lower-level constructor that accepts a pre-measured budget in bytes", but its signature takes `budget_max_mb: u32` (megabytes), not bytes. Internally it simply delegates to `Self::new()`, making it a pure alias. The comment is misleading because it suggests the method accepts bytes when it actually accepts megabytes.

**Fix:** Either update the comment or remove the method entirely (it's only used in tests and the tests could call `TenantQuotaLimiter::new()` directly):
```rust
/// Convenience alias for `new()`. Used primarily in tests to clarify intent
/// when constructing limiters with small byte-scale budgets.
#[doc(hidden)]
pub fn new_with_budget(
    budget_max_mb: u32,
    tenant_budget_used: Arc<AtomicUsize>,
    inner: InstanceHardLimiter,
) -> Self {
    Self::new(budget_max_mb, tenant_budget_used, inner)
}
```

---

## Info

### IN-01: Outdated doc comment references deprecated `async_support(true)`

**File:** `crates/jadepaw-wasm/src/engine.rs:13`
**Issue:** The module doc header states that the Engine configuration includes `async_support(true)`, but this method is deprecated in wasmtime 45.0 (it is a no-op since async is always supported when the `async` feature is enabled). The code correctly does NOT call it. The comment is misleading to readers who might wonder why it's mentioned but not called.

**Fix:** Update line 13:
```rust
//! - Async support is built-in (wasmtime 45.0+; no explicit `async_support` call needed)
```

### IN-02: `context` parameter only applied to initial messages, not re-injected per turn

**File:** `crates/jadepaw-agent/src/loop.rs:72-74`
**Issue:** The `context` (e.g., skill instructions, system context) is embedded in the initial user message via `build_initial_messages()` but is never re-injected into the conversation history in subsequent turns. On Turn 2+, the LLM sees only `{system_prompt, user_message_with_context, assistant_msgs...}`. As the conversation grows, the initial context may lose prominence relative to more recent assistant messages. This is a design choice for v1 but should be documented.

**Fix:** Update the `react_loop` doc comment to clarify this behavior:
```rust
/// - `context` is embedded in the first user message only (not re-injected
///   each turn). For long-running agent sessions, skill instructions carried
///   in the system_prompt are preferred over context for persistent guidance.
```

### IN-03: `log_message` host function silently maps `debug`/`trace` levels to `info`

**File:** `crates/jadepaw-wasm/src/host/logging.rs:95-99`
**Issue:** The level routing match only handles `"error"` and `"warn"`. All other level strings — including `"debug"`, `"trace"`, `"off"`, and any guest-side typos — fall through to `info!`. This means a guest that intentionally logs at `"debug"` level to reduce noise will still emit `info`-level events in production, potentially inflating log volume.

**Fix:** Add `"debug"` and `"trace"` routing arms, and emit a warning for unrecognized levels:
```rust
match level {
    "error" => error!(%session_id, "guest: {}", message),
    "warn" => warn!(%session_id, "guest: {}", message),
    "info" => info!(%session_id, "guest: {}", message),
    "debug" => debug!(%session_id, "guest: {}", message),
    "trace" => trace!(%session_id, "guest: {}", message),
    unknown => {
        warn!(%session_id, "guest used unrecognized log level '{}', defaulting to info", unknown);
        info!(%session_id, "guest: {}", message);
    }
}
```

### IN-04: `domain_matches` internal wildcard limitations not documented

**File:** `crates/jadepaw-wasm/src/capability/mod.rs:98-124`
**Issue:** The `domain_matches` method only supports `*` as a bare wildcard (`"*"`) or as a prefix wildcard (`"*.example.com"`). A pattern like `"api.*.com"` or `"*.svc.*.internal"` would not match because internal `*` segments are not processed. `DomainPattern` accepts arbitrary strings, so a user could configure an unsupported pattern and get unexpected denials.

**Fix:** Add a doc comment to `DomainPattern` documenting the supported pattern syntax:
```rust
/// Supported patterns:
/// - `"*"` — matches any domain
/// - `"*.example.com"` — matches single-subdomain wildcards (e.g., `api.example.com`)
/// - `"exact.domain.com"` — exact match only
///
/// Note: internal wildcards (e.g., `"api.*.com"`) are not yet supported.
/// Multi-level subdomain matching requires multiple patterns.
```

### IN-05: `normalize_path` uses `PathBuf::from("..")` as sentinel for over-long paths

**File:** `crates/jadepaw-wasm/src/path.rs:44-47`
**Issue:** When a guest path exceeds `MAX_PATH_LEN` (4096 bytes), `normalize_path` returns `PathBuf::from("..")` as a sentinel. The sentinel causes `validate_sandbox_path` to fail the prefix check (correctly rejecting the operation), but the sentinel value does not carry any information about the original over-long path. Operators debugging "path validation failed" errors in logs would see `..` as the rejected path rather than the actual (truncation-worthy) guest input. Fortunately `validate_sandbox_path` does log the original guest path in the error message (`guest_path.to_string()` is the full input), so this is adequate for troubleshooting. The sentinel pattern is unusual but functionally correct.

**Fix:** No code change needed — consider adding a comment to the sentinel return noting that the original path is logged by `validate_sandbox_path`:
```rust
if path.len() > MAX_PATH_LEN {
    // Sentinel: causes validate_sandbox_path to reject. The caller
    // (validate_sandbox_path) logs the original untruncated guest_path.
    return PathBuf::from("..");
}
```

---

_Reviewed: 2026-06-02T08:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_