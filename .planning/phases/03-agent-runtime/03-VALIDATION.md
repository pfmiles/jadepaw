---
phase: 03
slug: agent-runtime
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-06-01
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) + tokio::test |
| **Config file** | none — cargo test discovers tests automatically |
| **Quick run command** | `cargo test -p jadepaw-agent -p jadepaw-core` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5 seconds (unit), ~30 seconds (workspace) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p jadepaw-agent -p jadepaw-core`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | AGENT-01 | — | N/A | unit | `cargo test -p jadepaw-agent test_react_loop_multiple_cycles` | ❌ W0 | ⬜ pending |
| 03-01-02 | 01 | 1 | AGENT-01 | — | N/A | integration | `cargo test -p jadepaw-agent test_agent_responds_with_reasoned_answer` | ❌ W0 | ⬜ pending |
| 03-01-03 | 01 | 1 | AGENT-01 | — | N/A | unit | `cargo test -p jadepaw-agent test_guest_export_evaluate_step_called` | ❌ W0 | ⬜ pending |
| 03-01-04 | 01 | 1 | AGENT-01 | — | N/A | unit | `cargo test -p jadepaw-agent test_fallback_when_guest_export_absent` | ❌ W0 | ⬜ pending |
| 03-02-01 | 02 | 2 | AGENT-03 | T-03-01 | SSE injection via tool output must be sanitized | unit | `cargo test -p jadepaw-agent test_sse_streaming_real_time` | ❌ W0 | ⬜ pending |
| 03-02-02 | 02 | 2 | AGENT-03 | — | N/A | unit | `cargo test -p jadepaw-agent test_sse_event_naming` | ❌ W0 | ⬜ pending |
| 03-02-03 | 02 | 2 | AGENT-03 | — | N/A | unit | `cargo test -p jadepaw-agent test_sse_done_event` | ❌ W0 | ⬜ pending |
| 03-03-01 | 03 | 2 | AGENT-04 | T-03-03 | Infinite loop prevented by max_iterations | unit | `cargo test -p jadepaw-agent test_max_iterations_termination` | ❌ W0 | ⬜ pending |
| 03-03-02 | 03 | 2 | AGENT-04 | T-03-03 | Wall-clock timeout enforced | unit | `cargo test -p jadepaw-agent test_wall_clock_timeout` | ❌ W0 | ⬜ pending |
| 03-03-03 | 03 | 2 | AGENT-04 | — | N/A | unit | `cargo test -p jadepaw-agent test_termination_error_message` | ❌ W0 | ⬜ pending |
| 03-03-04 | 03 | 2 | AGENT-04 | — | N/A | unit | `cargo test -p jadepaw-agent test_fuel_reset_per_turn` | ❌ W0 | ⬜ pending |
| 03-03-05 | 03 | 2 | AGENT-04 | — | N/A | integration | `cargo test -p jadepaw-agent test_wasm_limits_preserved` | ❌ W0 | ⬜ pending |
| — | 01 | 1 | — | — | N/A | unit | `cargo test -p jadepaw-agent test_run_agent_returns_structured_response` | ❌ W0 | ⬜ pending |
| — | 01 | 1 | — | — | N/A | integration | `cargo test -p jadepaw-agent test_programmatic_invocation` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/jadepaw-agent/tests/agent_loop.rs` — covers AGENT-01 loop integration
- [ ] `crates/jadepaw-agent/tests/sse_streaming.rs` — covers AGENT-03 streaming
- [ ] `crates/jadepaw-agent/tests/termination.rs` — covers AGENT-04 guards
- [ ] `crates/jadepaw-core/tests/agent_types.rs` — covers AgentRequest/Response serialization
- [ ] `crates/jadepaw-agent/Cargo.toml` — test dependencies (mock LLM server or wiremock)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Real LLM streaming against live API endpoint | AGENT-03 | Requires API key and network access | Set `OPENAI_API_KEY` and `OPENAI_BASE_URL` env vars, run `cargo test -p jadepaw-agent -- --ignored` |
| SSE streaming end-to-end with axum HTTP server | AGENT-03 | axum server integration deferred to Phase 7 | Verify programmatically via mpsc channel receiver in test harness |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending