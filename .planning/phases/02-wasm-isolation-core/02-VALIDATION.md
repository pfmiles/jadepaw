---
phase: 02
slug: wasm-isolation-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-30
---

# Phase 02 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `#[test]` + `#[tokio::test(flavor = "multi_thread")]` |
| **Config file** | `.config/nextest.toml` |
| **Quick run command** | `cargo test -p jadepaw-wasm --lib` |
| **Full suite command** | `cargo test -p jadepaw-wasm` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p jadepaw-wasm --lib`
- **After every plan wave:** Run `cargo test -p jadepaw-wasm`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-01 | 01 | 1 | SEC-01 | T-02-01 | Store-per-session: data from session A not visible in session B | integration | `cargo test -p jadepaw-wasm test_store_isolation -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-02 | 01 | 1 | SEC-02 | T-02-02 | Guest exceeding 64MB terminated with clear error | integration | `cargo test -p jadepaw-wasm test_memory_hard_cap -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-03 | 01 | 1 | SEC-02 | T-02-03 | Fuel exhaustion terminates with trap | integration | `cargo test -p jadepaw-wasm test_fuel_exhaustion -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-04 | 01 | 1 | SEC-02 | T-02-04 | Epoch interruption triggers async yield | integration | `cargo test -p jadepaw-wasm test_epoch_yield -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-05 | 01 | 1 | SEC-03 | T-02-05 | Path traversal `../../../etc/passwd` rejected before tool runs | unit | `cargo test -p jadepaw-wasm test_path_traversal_rejected -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-06 | 01 | 1 | SEC-03 | T-02-06 | Valid path within sandbox root accepted | unit | `cargo test -p jadepaw-wasm test_valid_path_accepted -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-07 | 01 | 1 | SEC-04 | T-02-07 | Tool not in capability whitelist rejected with permission error | integration | `cargo test -p jadepaw-wasm test_capability_denied -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-08 | 01 | 1 | SEC-04 | T-02-08 | Tool in whitelist allowed through | integration | `cargo test -p jadepaw-wasm test_capability_allowed -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-09 | 01 | 1 | Stress | T-02-09 | 1,000 concurrent sessions each within 64MB cap | stress | `cargo test -p jadepaw-wasm test_concurrent_sessions_stress -- --nocapture --ignored` | ❌ W0 | ⬜ pending |
| 02-01-10 | 01 | 1 | D-01 | — | HostFunctions trait is CI-verifiable, additive-only | unit | `cargo test -p jadepaw-core test_host_functions_trait -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-11 | 01 | 1 | D-07 | T-02-10 | InstanceHardLimiter returns Err() on >64MB (Store poisoned) | unit | `cargo test -p jadepaw-wasm test_instance_hard_limit_trap -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-12 | 01 | 1 | D-07 | T-02-11 | TenantQuotaLimiter returns Ok(false) on budget exceeded (recoverable) | unit | `cargo test -p jadepaw-wasm test_tenant_quota_recoverable -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-13 | 01 | 1 | D-10 | — | InstanceCapabilities struct in jadepaw-core with all required fields | unit | `cargo test -p jadepaw-core test_capabilities_struct -- --nocapture` | ❌ W0 | ⬜ pending |
| 02-01-14 | 01 | 1 | D-12 | — | Default-deny: unregistered capability returns false | unit | `cargo test -p jadepaw-wasm test_default_deny -- --nocapture` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/jadepaw-wasm/tests/` — integration tests covering isolation, memory cap, capability enforcement
- [ ] `crates/jadepaw-core/tests/` — trait and struct tests
- [ ] Test fixtures: minimal `guest_module.wasm` compiled to `wasm32-wasi` that exercises host function imports
- [ ] Test helper: `build_test_engine()` factory function with pooling+fuel+epoch config for test reuse
- [ ] `crates/jadepaw-core/src/host_functions.rs` — HostFunctions trait definition
- [ ] `crates/jadepaw-core/src/capabilities.rs` — InstanceCapabilities, PathPattern, DomainPattern types

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Guest module compilation pipeline (Rust → wasm32-wasi binary) | Phase-wide | Requires wasm32-wasi target installed; `rustc` toolchain check | `rustup target list --installed \| grep wasm32-wasi` |
| PoolingAllocator explicitly NOT using copy-on-write (CoW off by default) | SEC-01 | Requires runtime profiling to verify memory isolation | `cargo test -p jadepaw-wasm` with MIRI or Valgrind |
| 5ms P99 cold start benchmark | D-06 | Requires bencher with warm JIT (not a test assertion) | `cargo bench -p jadepaw-wasm --bench instantiation_latency` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending