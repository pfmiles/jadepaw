---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: Not started
last_updated: "2026-05-28T13:37:24.309Z"
progress:
  total_phases: 9
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# STATE: jadepaw

**Last updated:** 2026-05-28
**Project:** jadepaw — multi-tenant AI Agent runtime platform with WebAssembly isolation

## Project Reference

**Core Value:** 让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

**Current Focus:** Phase 1 — Project Foundation (workspace scaffold, crate structure, build system, CI)

**Key Constraints:**

- Rust + wasmtime + tokio (non-negotiable stack)
- Wasm hardware-level isolation (no fallback to process-level)
- Multi-tenancy designed from Day 1
- Web-first UI (unified local + remote)
- Open source, quality over speed

## Current Position

**Phase:** 1 — Project Foundation
**Plan:** Not yet created (TBD)
**Status:** Not started
**Progress:** 0/9 phases complete

```
Progress: [░░░░░░░░░░░░░░░░░░░░] 0%
```

## Performance Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Build time (clean) | < 5 min | — |
| Cold start latency (Wasm) | < 5ms P99 | — |
| Concurrent instances (single node) | > 10,000 | — |
| Memory per instance | < 64MB | — |
| Test coverage | > 80% | — |

## Accumulated Context

### Decisions Made

- Tech stack: Rust 2024 + wasmtime 38.0 + tokio + axum 0.8.4 (from research)
- LLM client: async-openai 0.34.0 (multi-provider via `Box<dyn Config>`)
- Frontend: HTMX 2.0.4 + SSE (no build step)
- Database: SQLx (SQLite for single-node, PostgreSQL for cluster)
- Observability: tracing + opentelemetry + metrics + Prometheus exporter
- Wasm pattern: Store-per-session, InstancePre pool, Fuel+Epoch both ON from Day 1
- Skill model: Data-driven configuration (not compiled Wasm), SKILL.md format
- Agent loop: Pure ReAct for v1 (hybrid planning deferred to v2)

### Pending Decisions

- WIT vs raw FFI for guest-host communication protocol
- Context window management strategy (exact compression algorithm)
- Phase 2+ frontend enhancement (Alpine.js evaluation for reactive forms)

### Open Questions

- Q-001: Hybrid Planning prompt engineering (Phase 2 research)
- Anthropic API access path for non-OpenAI-compatible providers
- Exact session migration protocol for cross-node state transfer (Phase 3)

### Todos

- [ ] Create Phase 1 plan (`/gsd-plan-phase 1`)

### Blockers

- None

## Session Continuity

**Last session:** 2026-05-28T13:37:24.301Z
**Next action:** Create Phase 1 plan with `/gsd-plan-phase 1`
**Context to restore:** Project is greenfield. No code written yet. Architecture and requirements fully defined in .planning/ directory.
