---
phase: 05
slug: session-memory
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-04
---

# Phase 05 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo-nextest |
| **Config file** | .config/nextest.toml |
| **Quick run command** | `cargo nextest run -p jadepaw-db -p jadepaw-agent` |
| **Full suite command** | `cargo nextest run --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run -p jadepaw-db -p jadepaw-agent`
- **After every plan wave:** Run `cargo nextest run --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 05-01-01 | 01 | 1 | MEM-01 | — | N/A | unit | `cargo nextest run -p jadepaw-agent` | ❌ W0 | ⬜ pending |
| 05-01-02 | 01 | 1 | MEM-01 | — | N/A | unit | `cargo nextest run -p jadepaw-agent` | ❌ W0 | ⬜ pending |
| 05-02-01 | 02 | 1 | MEM-02 | — | N/A | unit | `cargo nextest run -p jadepaw-db` | ❌ W0 | ⬜ pending |
| 05-02-02 | 02 | 1 | MEM-02 | — | N/A | integration | `cargo nextest run -p jadepaw-agent` | ❌ W0 | ⬜ pending |
| 05-03-01 | 03 | 2 | MEM-01, MEM-02 | — | N/A | integration | `cargo nextest run --workspace` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/jadepaw-db/tests/` — stubs for MEM-02 (session persistence)
- [ ] `crates/jadepaw-agent/tests/` — stubs for MEM-01 (context window)
- [ ] `crates/jadepaw-agent/tests/` — stubs for MEM-01+MEM-02 integration

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Mid-turn crash recovery | MEM-02 | Requires process kill mid-LLM-stream | Kill process during streaming, restart, verify session resumes from last turn boundary |
| Cross-session tenant isolation | MEM-02 | Requires concurrent session state verification | Open two sessions for different tenants, verify no cross-contamination in DB queries |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending