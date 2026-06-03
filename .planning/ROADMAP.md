# Roadmap: jadepaw

**Created:** 2026-05-28
**Granularity:** Fine (9 phases)
**Project Mode:** mvp
**Core Value:** 让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

## Phases

- [x] **Phase 1: Project Foundation** — Workspace scaffold, crate structure, build system, CI (completed 2026-05-28)
- [x] **Phase 2: Wasm Isolation Core** — Per-session Wasm sandbox with hardware-level tenant isolation (completed 2026-05-30)
- [x] **Phase 3: Agent Runtime** — ReAct execution loop with streaming output and termination guards (completed 2026-06-01)
- [ ] **Phase 4: Tool System** — MCP-compatible tool protocol with file RW and HTTP tools
- [ ] **Phase 5: Session Memory** — In-session context management and SQLite-based persistence
- [ ] **Phase 6: Skill System** — Declarative SKILL.md format with hot loading and runtime swapping
- [ ] **Phase 7: Web Chat UI** — Browser-based streaming chat interface via HTMX + SSE
- [ ] **Phase 8: Skill Management UI** — Web interface for listing, loading, and unloading skills
- [ ] **Phase 9: Observability** — Session-correlated tracing and Prometheus metrics

## Phase Details

### Phase 1: Project Foundation

**Goal:** Rust workspace is scaffolded with all planned crates, builds successfully, and CI is green.
**Mode:** mvp
**Depends on:** Nothing (initial phase)
**Requirements:** (none — infrastructure phase; all subsequent phases depend on it)
**Success Criteria** (what must be TRUE):

  1. `cargo build --workspace` succeeds from a clean checkout on any platform (macOS, Linux)
  2. Crate dependency graph matches the architectural build order: core → wasm → bus → agent → skill → gateway → server
  3. `cargo test --workspace` passes with no failures
  4. `cargo clippy --workspace -- -D warnings` passes with zero warnings
  5. CI pipeline (fmt + build + test + clippy) runs on every push and completes in under 5 minutes

**Plans:** 2/2 plans complete
Plans:

- [x] 01-01-PLAN.md — Workspace scaffold: root Cargo.toml with workspace.dependencies, all 7 crates with Cargo.toml and lib.rs docs, workspace smoke test, project config files (rustfmt, clippy, deny, editorconfig, gitattributes, gitignore)
- [x] 01-02-PLAN.md — CI pipeline (GitHub Actions gate + matrix + security audit), justfile task runner, pre-commit hook, nextest configuration

### Phase 2: Wasm Isolation Core

**Goal:** Every agent session runs in an isolated wasmtime Store with strict resource limits, and tool execution is mediated through a capability whitelist with sandboxed file access.
**Mode:** mvp
**Depends on:** Phase 1 (needs workspace and crate structure)
**Requirements:** SEC-01, SEC-02, SEC-03, SEC-04
**Success Criteria** (what must be TRUE):

  1. A developer can create a fresh wasmtime Store per session, load a guest module, and execute Wasm code — the Store and all its linear memory are destroyed on session end with no data leaking to the next session
  2. A guest exceeding 64MB memory allocation is terminated with a clear error; same for Fuel exhaustion (infinite loop detection) and Epoch interruption
  3. A guest calling a host tool with a path like `../../../etc/passwd` is rejected before the tool runs — only paths within the tenant's designated sandbox directory are accepted
  4. A guest attempting to use a tool not in its capability whitelist (e.g., `http_request` when only `file_read` is granted) is rejected with a permission error before any side effects occur
  5. Running 1,000 concurrent isolated sessions does not cause memory exhaustion (verified by stress test: each session stays within its 64MB cap)

**Plans:** 3/3 plans complete
Plans:
**Wave 1**

- [x] 02-01-PLAN.md — Engine factory, core types (HostFunctions trait, InstanceCapabilities), delegating chain ResourceLimiter, SessionState, epoch ticker

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 02-02-PLAN.md — Host functions (log_message, file_read, file_write) with capability enforcement, path validation, capability check methods on SessionState

**Wave 3** *(blocked on Wave 2 completion)*

- [x] 02-03-PLAN.md — Instance pool with lazy instantiation, Semaphore concurrency bound, DashMap session tracking, stress test (1,000 concurrent sessions)

### Phase 3: Agent Runtime

**Goal:** Users can send natural language messages to the agent and receive streaming responses through a ReAct execution loop that autonomously reasons and acts with built-in safety limits.
**Mode:** mvp
**Depends on:** Phase 2 (Agent loop runs inside Wasm isolation)
**Requirements:** AGENT-01, AGENT-03, AGENT-04
**Success Criteria** (what must be TRUE):

  1. A user sends a natural language query and the agent responds with a reasoned answer — the agent can issue multiple think→act→observe cycles before responding
  2. Response tokens stream to the caller in real time (SSE), not as a single batch after the full computation completes
  3. An agent stuck in a loop is terminated after exceeding the max iteration limit (configurable, default 20), and the caller receives a graceful termination message explaining why
  4. An agent that takes longer than the wall-clock timeout (configurable, default 5 minutes) is terminated with a timeout error message
  5. The agent can be invoked programmatically (API call or test harness) and returns structured results including the final answer and execution trace

**Plans:** 2/2 plans complete

Plans:
**Wave 1**

- [x] 03-01-PLAN.md — Core types (AgentRequest, AgentResponse, ReActStep, AgentTerminationReason, GuestExports trait), ReAct loop skeleton, termination guards (tokio::select!), run_agent() entry point, unit tests

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 03-02-PLAN.md — async-openai LLM integration (chat-completion feature, streaming), SSE event relay (create_sse_channel), loop wired to real Client<Box<dyn Config>>, streaming integration tests

**UI hint:** yes

### Phase 4: Tool System

**Goal:** The agent can use external tools registered via an MCP-compatible protocol — at minimum file read/write and HTTP requests — to accomplish tasks beyond pure reasoning.
**Mode:** mvp
**Depends on:** Phase 3 (tools extend the ReAct loop's action space)
**Requirements:** AGENT-02
**Success Criteria** (what must be TRUE):

  1. A developer registers a file_read tool with the agent, sends a query like "read the file /data/notes.txt and summarize it", and the agent calls the tool, reads the file, and returns the summary
  2. A developer registers an http_request tool, and the agent can fetch a public URL and process the response content
  3. Tools are registered through an MCP-compatible interface — a tool implemented for Claude Code should be usable by jadepaw with minimal adaptation
  4. A tool call that fails (e.g., file not found, HTTP 500) is reported back to the agent with structured error information, and the agent can adapt its next action accordingly

**Plans:** 3 plans

Plans:
**Wave 1** *(types + registry — no dependencies)*

- [ ] 04-01-PLAN.md — Tool abstraction layer: Tool trait, ToolResult, ToolDefinition in jadepaw-core; ToolRegistry with capability-gated dispatch in jadepaw-agent; is_error on ReActStep::Observation; http_request on HostFunctions

**Wave 2** *(tool impls — blocked on Wave 1 types)*

- [ ] 04-02-PLAN.md — Tool implementations: FileReadTool and FileWriteTool wrapping Wasm sandbox host fns; HttpRequestTool with reqwest HTTP client, SSRF IP-layer protection, 1MB body cap, 30s timeout; http_request_host_fn stub replaced with real HTTP

**Wave 3** *(integration — blocked on Wave 1 registry + Wave 2 impls)*

- [ ] 04-03-PLAN.md — ReAct loop integration: ToolRegistry dispatch replaces placeholder Observation; run_agent() accepts optional ToolRegistry; system prompt augmented with tool list; SSE observation events carry is_error

### Phase 5: Session Memory

**Goal:** Conversations persist within a session with automatic context window management, and sessions can be paused, persisted, and resumed later.
**Mode:** mvp
**Depends on:** Phase 3 (memory manages the agent's conversation context)
**Requirements:** MEM-01, MEM-02
**Success Criteria** (what must be TRUE):

  1. A user has a long conversation (>10 messages) and the agent remembers earlier context — when the conversation approaches the token limit, older messages are automatically compressed into a summary rather than truncated
  2. A user pauses a session (or the system crashes), and after restarting, the user can resume the session from exactly where it left off — all conversation history, agent state, and pending actions are preserved
  3. A user opens two sessions simultaneously — their contexts are fully isolated with no cross-contamination
  4. Session data is stored in SQLite (single-file, zero-config), and the database can be backed up by copying the file

**Plans:** 3 plans

### Phase 6: Skill System

**Goal:** Users can define agent behaviors through declarative SKILL.md files, load them at runtime, and swap between different skills without restarting the agent.
**Mode:** mvp
**Depends on:** Phase 3 (skills modify agent behavior), Phase 4 (skills can declare tool requirements)
**Requirements:** SKILL-01, SKILL-02
**Success Criteria** (what must be TRUE):

  1. A user creates a SKILL.md file (YAML frontmatter: name, description, tools, constraints + Markdown body: natural language instructions), places it in the skills directory, and the agent immediately adopts the described behavior on the next invocation
  2. A user loads a "code reviewer" skill, has a conversation using that skill, then swaps to a "data analyst" skill mid-session — the agent's behavior changes to match the new skill without restarting
  3. A user unloads a skill, and the agent reverts to its default behavior on the next invocation
  4. A SKILL.md file with invalid YAML frontmatter is rejected at load time with a clear error message indicating the parse failure location
  5. Multiple skills can be loaded simultaneously and the agent correctly merges their tool declarations and instruction contexts

**Plans:** 3 plans

### Phase 7: Web Chat UI

**Goal:** Users open a browser to `localhost:PORT` and have a full streaming chat conversation with their agent through a clean web interface.
**Mode:** mvp
**Depends on:** Phase 3 (needs agent runtime), Phase 6 (skill context enriches chat experience)
**Requirements:** UI-01
**Success Criteria** (what must be TRUE):

  1. User navigates to `http://localhost:PORT` and sees a chat interface with an input field and message history area
  2. User types a message and presses Enter — response tokens appear incrementally in the chat bubble as the agent generates them (streaming SSE)
  3. The chat interface handles markdown formatting in agent responses (code blocks, lists, bold, links) rendered correctly in the browser
  4. User can start a new conversation (clear history) and maintain multiple concurrent chat sessions via different browser tabs or session IDs
  5. The chat interface works identically whether jadepaw is running on localhost or deployed to a remote server (same UI code)

**Plans:** 3 plans

**UI hint:** yes

### Phase 8: Skill Management UI

**Goal:** Users can view their loaded skills, list available skills, load new ones, and unload existing ones — all from the web interface.
**Mode:** mvp
**Depends on:** Phase 6 (skill system must exist), Phase 7 (UI framework is in place)
**Requirements:** UI-02
**Success Criteria** (what must be TRUE):

  1. User sees a sidebar or panel in the web interface listing all currently loaded skills with their names, descriptions, and status (active/inactive)
  2. User clicks "Load Skill" and selects a SKILL.md file from the skills directory — the skill loads and appears in the list immediately
  3. User clicks "Unload" on a loaded skill — the skill is removed from the agent's context and disappears from the list
  4. The skill list persists across browser refreshes and reflects the actual runtime state of the agent (not a cached view)

**Plans:** 3 plans

**UI hint:** yes

### Phase 9: Observability

**Goal:** Operators can trace every operation through the system with session-correlated IDs and monitor key metrics via a Prometheus endpoint.
**Mode:** mvp
**Depends on:** Phase 3 (agent generates traceable operations), Phase 7 (web server exposes metrics endpoint)
**Requirements:** OBS-01, OBS-02
**Success Criteria** (what must be TRUE):

  1. Every log line, span, and error includes both `session_id` and `instance_id` — an operator can grep for a session ID and see the complete lifecycle of that session across all system components
  2. Prometheus metrics endpoint (`/metrics`) exposes at minimum: active instance count, total memory usage, request latency histogram, error rate by type, and session count
  3. A developer can run `curl localhost:PORT/metrics` and see real-time counters and gauges updating as sessions are created and destroyed
  4. Structured tracing spans nest correctly: a tool call span is a child of the agent reasoning span, which is a child of the session span

**Plans:** 3 plans

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Project Foundation | 2/2 | Complete   | 2026-05-28 |
| 2. Wasm Isolation Core | 3/3 | Complete    | 2026-05-30 |
| 3. Agent Runtime | 2/2 | Complete    | 2026-06-01 |
| 4. Tool System | 0/3 | Planned | — |
| 5. Session Memory | 0/? | Not started | — |
| 6. Skill System | 0/? | Not started | — |
| 7. Web Chat UI | 0/? | Not started | — |
| 8. Skill Management UI | 0/? | Not started | — |
| 9. Observability | 0/? | Not started | — |