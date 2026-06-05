---
phase: 06
slug: skill-system
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-05
---

# Phase 06 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo-nextest (Rust) |
| **Config file** | `.config/nextest.toml` |
| **Quick run command** | `cargo nextest run -p jadepaw-skill` |
| **Full suite command** | `cargo nextest run --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run -p jadepaw-skill`
- **After every plan wave:** Run `cargo nextest run --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 01 | 1 | SKILL-01 | — | N/A | unit | `cargo nextest run -p jadepaw-skill skill_manifest_parse` | ⬜ W0 | ⬜ pending |
| 06-01-02 | 01 | 1 | SKILL-01 | — | N/A | unit | `cargo nextest run -p jadepaw-skill skill_manifest_validation` | ⬜ W0 | ⬜ pending |
| 06-02-01 | 02 | 1 | SKILL-02 | — | N/A | unit | `cargo nextest run -p jadepaw-skill skill_manager_load` | ⬜ W0 | ⬜ pending |
| 06-02-02 | 02 | 1 | SKILL-02 | — | N/A | integration | `cargo nextest run -p jadepaw-agent skill_swap_mid_session` | ⬜ W0 | ⬜ pending |
| 06-03-01 | 03 | 2 | SKILL-02 | — | N/A | unit | `cargo nextest run -p jadepaw-skill skill_index_sync` | ⬜ W0 | ⬜ pending |
| 06-03-02 | 03 | 2 | SKILL-01 | — | N/A | integration | `cargo nextest run -p jadepaw-skill skill_scan_directory` | ⬜ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/jadepaw-skill/tests/` — test stubs for skill manifest parsing and validation
- [ ] `crates/jadepaw-agent/tests/` — integration test stubs for skill swap mid-session

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| SKILL.md hot-reload (file system detection) | SKILL-02 | File system watcher behavior varies by OS | Place SKILL.md in skills dir, verify agent picks up on next invocation via API |
| Multi-tenant skill isolation | SKILL-01 | Requires multi-tenant setup | Load same skill name for two tenants, verify no cross-contamination |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending