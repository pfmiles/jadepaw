---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
last_updated: "2026-06-04T16:09:41.250Z"
progress:
  total_phases: 9
  completed_phases: 4
  total_plans: 12
  completed_plans: 10
  percent: 44
---

# STATE: jadepaw

**Last updated:** 2026-05-30
**Project:** jadepaw — multi-tenant AI Agent runtime platform with WebAssembly isolation

## Project Reference

**Core Value:** 让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

**Current Focus:** Phase 5 — session memory

**Key Constraints:**

- Rust + wasmtime + tokio (non-negotiable stack)
- Wasm hardware-level isolation (no fallback to process-level)
- Multi-tenancy designed from Day 1
- Web-first UI (unified local + remote)
- Open source, quality over speed

## Current Position

Phase: 04 (tool-system) — EXECUTING
Plan: 1 of 3
**Phase:** 5
**Plan:** Not started
**Status:** Ready to execute
**Progress:** 2/9 phases complete

```
Progress: [████░░░░░░░░░░░░░░░░░░] 22%
```

## Performance Metrics

| Metric | Target | Current |
|--------|--------|---------|
| Build time (clean) | < 5 min | — |
| Cold start latency (Wasm) | < 5ms P99 | ~0-2ms avg (Phase 2 benchmark) |
| Concurrent instances (single node) | > 10,000 | — |
| Memory per instance | < 64MB | — |
| Test coverage | > 80% | — |

## Accumulated Context

### Decisions Made

- Tech stack: Rust 2024 + wasmtime 45.0 + tokio + axum 0.8.4 (Phase 1, 2 validated)
- LLM client: async-openai 0.34.0 (multi-provider via `Box<dyn Config>`)
- Frontend: HTMX 2.0.4 + SSE (no build step)
- Database: SQLx (SQLite for single-node, PostgreSQL for cluster)
- Observability: tracing + opentelemetry + metrics + Prometheus exporter
- Wasm pattern: Store-per-session, InstancePre pool, Fuel+Epoch both ON from Day 1
- Skill model: Data-driven configuration (not compiled Wasm), SKILL.md format
- Agent loop: Pure ReAct for v1 (hybrid planning deferred to v2)
- ResourceLimiter: Delegating chain — InstanceHardLimiter (64MB Err) + TenantQuotaLimiter (budget Ok(false)) — Phase 2
- HostFunctions trait: async_trait in jadepaw-core, additive-only, capability-gated before I/O — Phase 2
- Path validation: normalize + canonicalize + sandbox prefix check, TOCTOU window documented — Phase 2
- InstancePool: Arc<InstancePre> + Store::new + Semaphore + DashMap, lazy instantiation — Phase 2
- Capability enforcement: Default deny, InstanceCapabilities with can_* checks, jadepaw namespace host fns — Phase 2

### Pending Decisions

- WIT vs raw FFI for guest-host communication protocol
- Context window management strategy (exact compression algorithm)
- Phase 2+ frontend enhancement (Alpine.js evaluation for reactive forms)

### Open Questions

- Q-001: Hybrid Planning prompt engineering (Phase 3 research)
- Anthropic API access path for non-OpenAI-compatible providers
- Exact session migration protocol for cross-node state transfer (Phase 3)

### Todos

- [ ] Discuss Phase 3 with `/gsd-discuss-phase 3`

### Blockers

- None

## Session Continuity

**Last session:** 2026-06-04T14:32:40.874Z
**Next action:** Discuss Phase 3 with `/gsd-discuss-phase 3`
**Context to restore:** Phase 2 (Wasm Isolation Core) complete — EngineFactory, ResourceLimiter chain, host functions with capability enforcement, path validation, InstancePool with Semaphore+DashMap. 3/3 plans done, 64 tests passing, security review passed (0 open threats). Phase 3 (Agent Runtime) ready to plan.
