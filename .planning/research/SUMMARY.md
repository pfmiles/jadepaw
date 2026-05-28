# Research Summary: jadepaw

**Generated:** 2026-05-28
**Sources:** STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md
**Overall Confidence:** HIGH

## Executive Summary

Jadepaw is a multi-tenant AI agent runtime that combines WebAssembly hardware-level isolation with a conversational skill creation model targeting non-developer users. The platform lets creators define agent behaviors through natural language dialogue, test them in Wasm sandbox previews, and one-click publish them as multi-tenant enterprise services — a path no competitor bridges.

The expert consensus across all four research streams converges on a **Rust + wasmtime + tokio + axum** stack, with a pre-warmed Wasm instance pool as the core architectural pattern. This pool achieves sub-5ms cold starts and provides pointer-level memory isolation between tenants — a genuine security moat against every existing agent platform (Claude Code, LangGraph, OpenAI GPTs, OpenClaw), all of which rely on OS-level or framework-level boundaries.

## Converged Stack

| Component | Choice | Rationale |
|-----------|--------|-----------|
| **Wasm Runtime** | wasmtime 38.0 | Pooling allocator, CVE process, Bytecode Alliance, benchmark leader (61.5) |
| **Web Framework** | axum 0.8.4 | Native SSE/WebSocket, tokio ecosystem, session extractors |
| **LLM Client** | async-openai 0.34.0 | Multi-provider via `Box<dyn Config>`, avoid langchain-rust |
| **Frontend** | HTMX 2.0.4 + SSE | No build step, SSE streaming, static files via tower-http |
| **Observability** | tracing + opentelemetry + metrics | Spans map to sessions, Prometheus exporter |
| **Database** | SQLx (SQLite→PostgreSQL) | Compile-time checked SQL, async, zero-config start |
| **Testing** | rstest + axum-test + testcontainers | Async fixtures, full-stack testing, container lifecycle |

**Rejected:** actix-web (own runtime conflicts with tokio), langchain-rust (framework fights custom Agent Loop), wasmer (security track record), wasm3 (interpreter-only, no JIT).

## Feature Priorities

### Table Stakes (must ship)
ReAct agent loop, tool calling with MCP compat, streaming output, short-term memory, skill format (SKILL.md), skill loading, Wasm instance isolation, Web Chat UI, basic permissions, session persistence.

### Differentiators (competitive advantage)
- **Interactive Skill Creation + Wasm Sandbox Preview**: The "aha moment" — no competitor offers safe preview of user-created skills
- **Personal → Enterprise Publish Pipeline**: One-click deploy from personal agent to multi-tenant service
- **Natural Language as Programming**: Non-developers create agent behaviors through conversation

### Anti-Features (deliberately excluded from MVP)
- Visual drag-and-drop workflow editor (NL + forms sufficient for v1)
- Centralized skill marketplace (Git-based distribution first)
- Hybrid planning (ship pure ReAct first, add planning layer post-validation)
- Native mobile apps (web-first)

## Architecture Blueprint

### Build Order (dependency-driven)
```
jadepaw-core → jadepaw-wasm → jadepaw-bus → jadepaw-agent → jadepaw-skill → jadepaw-gateway → jadepaw-server
```

### Three-Layer State Architecture
1. **Wasm Store** (ephemeral) — per-session, zeroed on instance reset, hardware-level tenant separation
2. **Redis** (session state) — keyed `session:{id}:*`, supports cross-node migration
3. **PostgreSQL** (tenant state) — configs, skills, users, with `tenant_id` column defense-in-depth

### Key Architectural Patterns
- **Store-per-session**: Engine/Module/Linker pooled, Store created fresh per session and dropped on end
- **InstancePre pool**: `DashMap<ModuleVersion, ArrayQueue<InstancePre>>` for zero-downtime upgrades
- **Sharded event bus**: `tokio::sync::broadcast` channels by tenant_id, `async-nats` for cross-node
- **Three-layer tool sandbox**: capability whitelist → CLONE_NEWUSER + chroot → seccomp BPF

## Critical Pitfalls

### Phase 1 Invariants (correctness, not optimization)
1. **Fuel + Epoch both ON from Day 1**: wasmtime defaults to OFF; multi-tenant fairness requires both
2. **Store-per-session, never pool Stores**: Pool Engine/Module/Linker, but Store must be fresh per tenant
3. **Guest value validation**: All host-call parameters validated before use (path normalization, size checks)
4. **StoreLimits explicit**: All wasmtime resource limits default to unlimited; set per-instance caps

### Phase 2 Invariants
5. **Agent loop 5-layer defense**: max_iterations + token_budget + loop_detection + wall_clock_timeout + cost_limit
6. **Prompt injection scanning for user Skills**: Content scanning, structured segregation, provenance tracking
7. **Instance pool explicit zeroing**: wasmtime's internal zeroing is best-effort, not contractual

### Phase 3 Invariants
8. **Spectre honesty**: Don't promise hardware-level isolation; for high-security tenants, offer dedicated hardware

## Unresolved Conflicts

None. The four researchers are well-aligned. Two areas where decisions are left open:

1. **Skill compilation model**: Skills are data-driven configurations loaded into a generic agent Wasm module, NOT compiled to Wasm bytecode. Skill "compilation" = prompt engineering + validation.
2. **Frontend for forms**: Phase 1 uses HTMX-only. Phase 2 evaluates Alpine.js if skill management forms need reactive state.

## Phase Structure Recommendation

| Phase | Focus | Key Deliverables |
|-------|-------|-----------------|
| **Phase 1** | Core Agent Runtime (MVP) | ReAct loop, tool calling, streaming, skills, Web Chat, Wasm isolation, session persistence |
| **Phase 2** | Creator Experience | Interactive skill creation, Wasm sandbox preview, hybrid planning, agent bus |
| **Phase 3** | Enterprise Platform | Multi-tenant routing, publish pipeline, auth/audit, cluster mode, skill DAG, multi-agent |

## Research Flags

Needs phase-specific research during planning:
- Phase 1: LLM prompt engineering for ReAct loop
- Phase 2: Interactive skill creation dialogue UX
- Phase 3: Multi-agent orchestration patterns

Standard patterns (skip research): Wasm instance pool, SSE/WebSocket streaming, MCP tool protocol, JWT/auth.

## Gaps

- Hybrid Planning prompt engineering (Q-001)
- Wasm guest-host communication protocol spec (WIT vs raw FFI)
- Context window management strategy
- Anthropic API access path for non-OpenAI-compatible providers