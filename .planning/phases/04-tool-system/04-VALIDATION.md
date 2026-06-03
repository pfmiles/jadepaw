---
phase: 04
slug: tool-system
status: draft
nyquist_compliant: true
wave_0_complete: true
created: 2026-06-03
---

# Phase 04 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo-nextest (Rust) |
| **Config file** | `.config/nextest.toml` (Phase 1) |
| **Quick run command** | `cargo nextest run -p jadepaw-core -p jadepaw-agent -p jadepaw-wasm` |
| **Full suite command** | `cargo nextest run --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo nextest run -p jadepaw-core -p jadepaw-agent -p jadepaw-wasm`
- **After every plan wave:** Run `cargo nextest run --workspace`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Wave 0 Decision

**Wave 0 test stubs are NOT created as separate files.** Instead, tests are written inline during execution within each task. The `<automated>` verify blocks in each plan run `cargo test -p <crate>` which discovers and runs both existing and newly-written tests at task completion time.

This decision was made because:
- Each plan's tasks already include `<automated>` verify commands that run the full crate test suite
- Creating separate Wave 0 stub files would add unnecessary file scaffolding that gets overwritten immediately
- The Nyquist sampling continuity requirement is satisfied by `cargo test` in the `<automated>` blocks of each task

The per-task verification map below documents test commands for traceability; actual test files are created by the executor inline.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|--------|
| 04-01-01 | 01 | 1 | AGENT-02 | T-04-01 | Tool trait with capability-gated dispatch | unit | `cargo nextest run -p jadepaw-core` | ⬜ pending |
| 04-01-02 | 01 | 1 | AGENT-02 | T-04-02 | ToolRegistry with can_call_tool() enforcement | unit | `cargo nextest run -p jadepaw-agent` | ⬜ pending |
| 04-01-03 | 01 | 1 | AGENT-02 | T-04-03 | ToolResult::Error with structured error codes | unit | `cargo nextest run -p jadepaw-core` | ⬜ pending |
| 04-02-01 | 02 | 2 | AGENT-02 | T-04-04 | file_read/write Tool impls with Wasm sandbox | unit | `cargo nextest run -p jadepaw-wasm` | ⬜ pending |
| 04-02-02 | 02 | 2 | AGENT-02 | T-04-05 | http_request host fn with SSRF protection | unit | `cargo nextest run -p jadepaw-wasm` | ⬜ pending |
| 04-03-01 | 03 | 3 | AGENT-02 | T-04-06 | ReAct loop tool dispatch + error observation | integration | `cargo nextest run -p jadepaw-agent` | ⬜ pending |
| 04-03-02 | 03 | 3 | AGENT-02 | T-04-07 | MCP-compatible tools/list + tools/call | integration | `cargo nextest run -p jadepaw-agent` | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| MCP wire format compatibility with external tools | AGENT-02 | Requires real MCP client for interop testing | Register a tool with MCP-style schema, verify `tools/list` output matches MCP spec |
| HTTP SSRF protection against DNS rebinding | AGENT-02 | DNS rebinding is accepted MVP risk per D-03 | Manual penetration test with rebinding attack scenario |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references (tests written inline during execution)
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** pending