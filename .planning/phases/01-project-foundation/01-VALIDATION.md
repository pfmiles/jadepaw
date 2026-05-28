---
phase: 01
slug: project-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-05-28
---

# Phase 01 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` / `cargo nextest` |
| **Config file** | `Cargo.toml` (workspace) |
| **Quick run command** | `cargo build --workspace` |
| **Full suite command** | `cargo nextest run --workspace && cargo test --doc --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo build --workspace && cargo test --workspace`
- **After every plan wave:** Run `cargo nextest run --workspace && cargo test --doc --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | SC-1 (build) | N/A | N/A | build | `cargo build --workspace` | ❌ W0 | ⬜ pending |
| 01-01-02 | 01 | 1 | SC-2 (dep graph) | N/A | N/A | integration | `cargo build --workspace` (verifies deps) | ❌ W0 | ⬜ pending |
| 01-01-03 | 01 | 1 | SC-3 (test) | N/A | N/A | test | `cargo test --workspace` | ❌ W0 | ⬜ pending |
| 01-01-04 | 01 | 1 | SC-4 (clippy) | N/A | N/A | lint | `cargo clippy --workspace -- -D warnings` | ❌ W0 | ⬜ pending |
| 01-01-05 | 01 | 1 | SC-5 (CI) | N/A | N/A | ci | CI pipeline green on push | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `Cargo.toml` — workspace manifest with all 7 member crates
- [ ] `crates/*/Cargo.toml` — per-crate manifests with correct dependency declarations
- [ ] `crates/*/src/lib.rs` — module-level `//!` doc comments
- [ ] `.github/workflows/ci.yml` — CI pipeline definition
- [ ] `justfile` — task runner recipes
- [ ] `cargo install cargo-nextest cargo-deny cargo-audit` — dev tooling installation

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CI completes in <5 min | SC-5 | CI runtime varies by runner | Push and observe Actions tab; verify matrix completes under 5 min |
| rustfmt style_edition=2024 compiles | SC-4 | Requires nightly rustfmt for 2024 edition | Verify `cargo fmt --check` passes in CI (stable rustfmt supports 2024 as of Rust 1.85) |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending