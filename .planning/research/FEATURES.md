# Feature Research

**Domain:** Multi-tenant AI Agent Runtime Platform (non-developer creator to enterprise service)
**Researched:** 2026-05-28
**Confidence:** HIGH

## Feature Landscape

### Table Stakes (Users Expect These)

Features users assume exist. Missing these = product feels incomplete.

#### Agent Execution

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Agent Loop (ReAct) | Every production agent platform uses think-act-observe loops. Claude Code uses it; LangGraph's core primitive is the agent loop; OpenAI Agents SDK follows the same pattern. Users expect an agent that reasons and acts iteratively. | MEDIUM | jadepaw already plans this as REQ-AGENT-001 (hybrid planning+ReAct). The hybrid approach (plan steps then ReAct within each) is a differentiator on top of the table-stakes loop. |
| Tool Calling (Function Calling) | Can't build an agent without it. MCP has become the industry standard protocol -- Claude Code, OpenAI, LangChain, and the open Agent Skills standard all support it. | MEDIUM | REQ-AGENT-002 already specifies MCP-compatible protocol. The key complexity is bridging Wasm instances to host-mediated tool execution. |
| Streaming (Token-level) | Users expect to see agent thinking in real time. Claude Code streams every tool call and response. LangGraph supports multi-mode streaming (updates, messages, custom, debug). Non-streaming agents feel broken. | LOW | REQ-AGENT-004 covers this. Implementation is straightforward with SSE/WebSocket from the Rust host. |
| Multi-turn Conversation | Agents must maintain conversation context within a session. This is the most basic form of "memory." Claude Code sessions persist automatically. LangGraph uses thread-scoped checkpoints. | LOW | REQ-MEMORY-001 covers short-term memory. The complexity is in context window management, not the conversation loop itself. |

#### Skill/Plugin System

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Declarative Skill Definition | Claude Code established the SKILL.md pattern (YAML frontmatter + Markdown instructions) as an open standard (agentskills.io). OpenAI GPTs, Poe bots, and others all use some form of declarative agent definition. Users expect to define agent behavior via structured config, not code. | LOW | REQ-SKILL-001 covers this. The Agent Skills standard provides a proven format. |
| Skill Hot-Loading | Claude Code detects skill changes live without restart. Skills load progressively (metadata on startup, body on activation). Users iterating on agent behavior expect instant feedback. | MEDIUM | REQ-SKILL-002. Wasm pre-init pool enables this naturally -- swap skill context without destroying the instance. |
| Tool Discovery/Availability | Skills need access to tools. In Claude Code, tools come from built-ins + MCP servers. The Agent Skills standard includes an `allowed-tools` field. Users expect skills to declare and have tools available. | LOW | REQ-AGENT-002 + REQ-SECURITY-003 (capability whitelist) cover this together. |

#### Memory & Context

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Session Persistence | Users close a chat and come back later -- the agent must remember. Every platform (ChatGPT, Claude, LangGraph checkpoints) does this. | MEDIUM | REQ-MEMORY-003. Complexity comes from cross-node migration in cluster mode, not single-instance persistence. |
| Context Window Management | LLMs have finite context. Without automatic summarization/compaction, agents fail on long conversations. Claude Code has `/compact`. LangGraph provides message filtering. | MEDIUM | Part of REQ-MEMORY-001. Various techniques: sliding window, summarization, trimming -- need to pick one. |

#### Human-in-the-Loop

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Tool Execution Approval | Claude Code has a tiered permission system (read-only auto-approved, bash/file-write ask). LangGraph has `interrupt()` for pausing at any point. Users expect safety gates before the agent takes destructive actions. | MEDIUM | REQ-HITL-001 covers this. The 4-tier policy model (never/only-write/high-confidence/always) is well-scoped. |
| Execution Pause/Resume | Interrupt capability -- LangGraph's `interrupt()` is the reference pattern. Claude Code hooks can block PreToolUse. Users need to stop and redirect the agent mid-execution. | MEDIUM | REQ-HITL-002. More complex than simple approval because it requires state serialization at arbitrary points. |

#### Enterprise Features

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Multi-tenancy (Data Isolation) | If you sell to enterprises, every tenant's data must be fully isolated. This is non-negotiable. Claude Code's enterprise offering has managed settings with organization-wide policies. | HIGH | REQ-SECURITY-001 (Wasm instance isolation) is the cornerstone. jadepaw's Wasm linear-memory model is a genuine architectural advantage here. |
| API Key / JWT Authentication | Every SaaS product has this. Table stakes for any multi-user platform. | LOW | REQ-SECURITY-005 covers the 3-layer auth model. |
| Audit Logging | SOC 2, GDPR, and enterprise compliance all require it. Claude Code records all tool calls. LangGraph traces every node execution. | MEDIUM | REQ-OBSERVABILITY-002. The complexity is in parameter sanitization to avoid logging PII. |

#### Non-Developer UX

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Web Chat Interface | If non-developers are the target, a terminal/CLI is unacceptable. ChatGPT, Poe, Claude.ai all prove the chat interface is the minimum viable UX. | LOW | REQ-UI-001. Built-in web server with streaming chat is the right approach. |
| Skill Preview/Testing | Before publishing an agent, users need to test it. Claude Code's Wasm-free preview is just running it; GPT Builder has a preview panel. Wasm sandbox preview (jadepaw's REQ-SKILL-003) is actually a differentiator. | MEDIUM | Overlaps with differentiator -- the sandbox preview itself is novel, but "test before publish" is expected. |

### Differentiators (Competitive Advantage)

Features that set the product apart. Not required, but valuable.

#### Agent Execution

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Hybrid Planning + ReAct Loop | Most platforms use either pure ReAct (Claude Code) or pure planning (early AutoGPT). The hybrid approach -- high-level plan then ReAct within each step, with local replanning on deviation -- gives users visibility into progress ("Step 2 of 5: aggregating data") while keeping execution flexible. | HIGH | REQ-AGENT-001. This is genuinely novel. The complexity is in the planning prompt engineering (Q-001) and replanning trigger logic. Start simple: 3-7 step plans with a "drift detector" that compares actual vs expected state. |
| JIT Trace Caching (Hot Path Compilation) | When the same tool sequence runs repeatedly (e.g., "query DB -> format -> return"), skip the LLM entirely and execute directly. Mentioned in the architecture doc as "JIT固化引擎." No other platform does this -- it's a Wasm-native optimization. | VERY HIGH | Defer past MVP. This is a Phase 3/4 feature. Requires production traffic patterns to identify hot paths. |

#### Skill/Plugin System

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Interactive Skill Creation (Skill Bootstrap) | Instead of writing SKILL.md by hand, users converse with a built-in agent that guides them through describing what they want, extracts intent, generates a structured skill draft, and lets them test in a Wasm sandbox. This is the "agent's first skill is helping you write skills" concept. Claude Code has no equivalent -- you must write SKILL.md manually. GPT Builder does something similar but without sandbox preview. | HIGH | REQ-SKILL-003. This is the core differentiator. The 5-step flow (dialogue -> intent extraction -> draft -> sandbox preview -> iterate) requires careful UX design. The Wasm sandbox preview is the moat. |
| Wasm Sandbox Skill Preview | The unique ability to safely test an incomplete/ buggy skill in an isolated Wasm instance before deploying it. ChatGPT's GPT Builder lets you test, but in their cloud -- not isolated. Claude Code has no skill preview at all. This is only possible because of the Wasm architecture. | HIGH | Part of REQ-SKILL-003. Requires a "preview mode" with restricted tool access (dry-run only), separate from production instances. |
| Personal-to-Enterprise Publish | Users create an agent for themselves, refine it through conversation, then one-click publish it as a multi-tenant enterprise service. No other platform bridges this gap: Claude Code is personal-only, enterprise AI platforms are top-down provisioned. | HIGH | REQ-DEPLOY-003. The complexity is in the publishing pipeline: skill metadata extraction, tenant provisioning, API gateway configuration. But it's the signature feature. |
| Skill Composition (DAG Workflows) | Multiple skills chained into a workflow via the host message bus. OpenClaw has channel pipelines but not skill-level composition. LangGraph can do this but requires code. jadepaw's approach -- Wasm instance wiring via host bus -- is architecturally cleaner. | HIGH | REQ-SKILL-005. Defer DAG editor UI past v1 -- start with simple linear composition and add branching later. |
| Git-based Skill Distribution | Skills are version-controlled Markdown/YAML files. Distribution via git repos, not a centralized marketplace. Claude Code's plugin marketplace is centralized. jadepaw's git-based approach is more aligned with open-source and avoids marketplace operational burden. | LOW | REQ-UI-003. The "market" is just git repos with a discovery UI. Much simpler to build than a centralized marketplace. |

#### Multi-Agent Orchestration

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Router-to-Specialist Delegation | A main agent decomposes tasks and delegates to specialized sub-agents, each with their own tools and context windows. Claude Code has subagents. LangGraph has supervisor patterns. jadepaw's Wasm-native approach means each specialist runs in its own isolated instance with hardware-level boundaries -- not just context isolation. | HIGH | REQ-AGENT-003. The architectural question (Q-005) is whether specialists run in the same Wasm instance or separate ones. Separate instances is correct for security but adds message-passing complexity. |
| Wasm Instance Topology for Multi-Agent | Running each specialist agent in its own Wasm instance creates a natural microservices-style topology. This is unique -- no other platform has per-agent hardware isolation. Enables true multi-tenant safety for agent-to-agent delegation. | VERY HIGH | REQ-AGENT-003 + REQ-SKILL-005. The complexity is in instance orchestration (spawning, messaging, lifecycle). Defer complex topologies past MVP -- start with 2-level hierarchy (router + specialists). |

#### Memory & Context

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Per-Tenant Long-Term Memory | Cross-session memory scoped to tenants, not just users. An enterprise tenant's agents learn from all interactions across their organization (with permission boundaries). LangGraph's Store supports namespaced long-term memory. Claude Code sessions are per-user. jadepaw can do org-level memory. | MEDIUM | REQ-MEMORY-002. The per-tenant scoping is the differentiator, not the memory mechanism itself. Use a vector store (pgvector/Qdrant) with tenant ID as namespace prefix. |
| Memory Extraction in Background | Extract key facts from sessions asynchronously, not blocking the conversation. LangGraph supports both "hot path" and "background" memory writing. This avoids slowing down the agent while still building long-term knowledge. | MEDIUM | Part of REQ-MEMORY-002. Defer background extraction past MVP -- start with session-end extraction or explicit user save. |

#### Enterprise Features

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Wasm Hardware-Level Multi-Tenancy | Unlike every other platform that uses OS-level or process-level isolation, jadepaw's Wasm linear memory model provides pointer-level isolation. This is a security differentiator for enterprise: "each tenant agent runs in a memory sandbox where pointers cannot escape." | HIGH | REQ-SECURITY-001. This is the architectural moat. Already core to the design. |
| Three-Layer Auth Model | API Key / JWT (access) -> Session Token (agent instance) -> Runtime Permission Check (tool execution). More granular than typical SaaS auth. Claude Code has permissions but not this layered model. | MEDIUM | REQ-SECURITY-005. The runtime permission check at the tool execution level is what makes it interesting -- permissions are checked at the point of action, not just at session start. |
| Instance-Level Resource Quotas | Per-tenant and per-instance resource limits (memory, CPU, network bandwidth, max tool timeout). Enterprise customers need predictable costs and isolation from noisy neighbors. | MEDIUM | Part of the architecture doc's resource management. Build quotas into the instance pool from Day 1. |

#### Non-Developer UX

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Conversational Skill Builder | "I want an agent that monitors my GitHub repos and sends me a daily Slack digest." The built-in agent guides the user through clarifying questions, generates the skill, and offers to test it. This is what makes the product accessible to non-developers -- they never touch YAML unless they want to. | HIGH | REQ-SKILL-003 + REQ-UI-002. The UX challenge is making the conversation feel guided but not constrained. Reference: ChatGPT's GPT Builder conversation flow, but add Wasm preview. |
| Hybrid Skill Editing (Chat + Form) | After conversational creation, users can refine via a structured form for precise control. This dual-mode editing (conversation for creation, form for refinement) covers both the "I don't know what I want" and "I know exactly what I want" user journeys. | MEDIUM | REQ-UI-002 explicitly calls for this hybrid approach. Form-based editing is much simpler to implement than conversational -- build the form first and add conversational creation on top. |
| Unified Local + Remote UI | `jadepaw serve` on localhost uses the same web UI as the enterprise deployment. Users have one consistent experience whether tinkering on their laptop or managing their published service. | LOW | REQ-UI-001. This is a design constraint already baked into the architecture (built-in web server). |

### Anti-Features (Commonly Requested, Often Problematic)

Features that seem good but create problems.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Visual Drag-and-Drop Workflow Editor | Non-developers "need" a visual editor to compose skills into workflows. Competitors like n8n and Dify lean heavily on this. | (1) Visual workflow editors are huge engineering investments. (2) Non-developers actually struggle with them -- the "boxes and arrows" metaphor is a developer mental model. (3) jadepaw's core value is natural language programming -- a visual editor undermines that thesis. | Natural language workflow composition: "Take the output from the GitHub skill and feed it to the Slack skill, running every morning at 9am." The agent translates this into a DAG. Form-based refinement for precision. Already in scope as REQ-SKILL-005 but implemented via NL, not drag-and-drop. |
| Centralized Plugin Marketplace | A marketplace where developers publish and monetize skills. GPT Store model. | (1) Marketplace operations (review, moderation, payments, takedown) are a full-time business. (2) Creates platform risk -- what if a popular skill has a security vulnerability? (3) jadepaw is an open-source project, not a platform business. | Git-based distribution with a discovery UI (REQ-UI-003). Skills are just files in git repos. Discovery via tags, search, ratings -- but installation is `git clone` or URL import. Operational burden near zero. |
| Real-Time Collaboration (Multiplayer Agent Editing) | "Google Docs for agents" -- multiple users co-creating a skill simultaneously. | (1) CRDT/OT is extremely complex to implement correctly. (2) The use case is unclear -- how often do multiple people need to edit the same skill at the same time? (3) jadepaw's skill format is git-friendly, so async collaboration via PRs works naturally. | Async collaboration via git. Skill version management (REQ-SKILL-004) with branches/PRs is sufficient. |
| Built-in LLM Provider | Running a local LLM inside jadepaw instead of calling external APIs. | (1) Running LLMs is a different product category. (2) Wasm cannot efficiently run LLM inference. (3) Would require bundling model weights, GPU support, etc. | jadepaw is LLM-agnostic. It calls external APIs (Anthropic, OpenAI, any OpenAI-compatible endpoint). Configuration specifies the provider. No opinion on which LLM -- that's the user's choice. |
| Mobile App | "I want to chat with my agents on my phone." | (1) Web-first means the web UI already works on mobile browsers. (2) A native app duplicates effort for marginal UX gain. (3) Out of scope per PROJECT.md. | Progressive Web App (PWA) as a potential v2 enhancement. Web-responsive design from the start. |
| Multi-Language SDK | Developers want SDKs in Python, JS, Go to build agents programmatically. | (1) jadepaw's target user is non-developers -- SDKs serve a different audience. (2) The REST API + Web UI is the primary interface. (3) SDK maintenance burden across languages is high. | REST API with OpenAPI spec. Any language can call it. If there's demand, community-contributed SDKs can emerge later. The Claude Agent SDK is a good reference for what a programmatic API looks like, but it's a v2+ concern. |
| Agent-to-Agent Marketplace Transactions | Agents autonomously discovering and paying each other for services. "AI agent economy." | (1) Crypto/web3 adjacent -- reputational risk. (2) Authentication and billing for agent-to-agent transactions is an unsolved problem. (3) Distracts from the core value prop. | Skill discovery via git repos. Free and open by default. Enterprise features (SSO, RBAC) for paid deployments. |

## Feature Dependencies

```
Agent Loop (REQ-AGENT-001)
    |-- requires --> Tool Calling (REQ-AGENT-002)
    |-- requires --> Streaming (REQ-AGENT-004)
    |-- requires --> Short-Term Memory (REQ-MEMORY-001)
    |
    |-- enhanced by --> Hybrid Planning (differentiator)
    |
    +-- Skill System (REQ-SKILL-001/002)
            |-- requires --> Skill Definition Format (REQ-SKILL-001)
            |-- requires --> Tool Calling (REQ-AGENT-002)
            |
            +-- Interactive Skill Creation (REQ-SKILL-003)
            |       |-- requires --> Web Chat UI (REQ-UI-001)
            |       |-- requires --> Wasm Sandbox Preview (differentiator)
            |       |-- requires --> Agent Loop (REQ-AGENT-001)
            |
            +-- Skill Composition (REQ-SKILL-005)
            |       |-- requires --> Multi-Agent Orchestration (REQ-AGENT-003)
            |       |-- requires --> Host Message Bus (architecture)
            |
            +-- Personal-to-Enterprise Publish (REQ-DEPLOY-003)
                    |-- requires --> Multi-Tenancy (REQ-SECURITY-001)
                    |-- requires --> Auth Model (REQ-SECURITY-005)
                    |-- requires --> Skill Version Management (REQ-SKILL-004)

Multi-Agent Orchestration (REQ-AGENT-003)
    |-- requires --> Agent Loop (REQ-AGENT-001)
    |-- requires --> Wasm Instance Management (REQ-SECURITY-001)
    |-- requires --> Host Message Bus (architecture)

Long-Term Memory (REQ-MEMORY-002)
    |-- requires --> Multi-Tenancy (REQ-SECURITY-001) [for tenant-scoped storage]
    |-- enhanced by --> Background Extraction (differentiator)

Human-in-the-Loop (REQ-HITL-001/002)
    |-- requires --> Tool Calling (REQ-AGENT-002)
    |-- requires --> Streaming (REQ-AGENT-004) [to surface approval requests]

Enterprise Publish (REQ-DEPLOY-003)
    |-- requires --> All Security features (REQ-SECURITY-001 through 005)
    |-- requires --> All Observability features (REQ-OBSERVABILITY-001 through 003)
    |-- requires --> Skill System (REQ-SKILL-001/002)
    |
    +-- Cluster Mode (REQ-DEPLOY-002)
            |-- requires --> Session State Migration (REQ-MEMORY-003)
            |-- requires --> Redis + Object Storage (infrastructure)

Audit Logging (REQ-OBSERVABILITY-002)
    |-- enhances --> Enterprise Publish (REQ-DEPLOY-003)
    |-- enhances --> Security Posture (REQ-SECURITY-001)

JIT Trace Caching (differentiator)
    |-- requires --> Production Traffic Patterns
    |-- conflicts with --> Frequent Skill Updates (cached traces invalidate)
    Defer past MVP.
```

### Dependency Notes

- **Agent Loop requires Tool Calling**: The ReAct loop fundamentally depends on think-act-observe, and "act" = tool execution. Cannot build the loop without at least a stub tool system.
- **Interactive Skill Creation requires Web Chat + Agent Loop**: The skill creation agent is itself an agent running the loop, exposed through the chat UI.
- **Skill Composition requires Multi-Agent Orchestration**: Composing skills into DAG workflows means each skill node may run as a specialist agent. The orchestration layer routes between them.
- **Personal-to-Enterprise Publish requires everything**: This is the "capstone" feature. It cannot ship until multi-tenancy, auth, audit, and skill packaging are all solid.
- **Long-Term Memory enhanced by Background Extraction**: Long-term memory without automatic extraction still adds value (manual save). Background extraction makes it seamless but isn't a hard blocker.

## MVP Definition

### Launch With (v1) -- The Single-User Local Agent

Minimum viable product -- what's needed to validate the core thesis that non-developers can create agents via natural language.

- [ ] **Agent Loop (REQ-AGENT-001)** -- Basic ReAct loop with tool calling. Defer the hybrid planning enhancement to v1.1; pure ReAct is simpler and proven (Claude Code uses it). The hybrid planner adds value but isn't needed to validate the core thesis.
- [ ] **Tool Calling (REQ-AGENT-002)** -- MCP-compatible protocol. MVP tools: file read/write and HTTP requests. These two cover 80% of initial use cases.
- [ ] **Streaming (REQ-AGENT-004)** -- Token-level SSE streaming to the Web Chat. Non-negotiable for UX.
- [ ] **Short-Term Memory (REQ-MEMORY-001)** -- In-session conversation context with basic window management (sliding window or simple summarization).
- [ ] **Skill Definition Format (REQ-SKILL-001)** -- The Agent Skills-compatible SKILL.md format. Even if MVP users write these by hand (no interactive creation yet), the format must exist.
- [ ] **Skill Loading (REQ-SKILL-002)** -- Load and execute skills from the filesystem. Hot-reloading for development iteration.
- [ ] **Wasm Instance Isolation (REQ-SECURITY-001)** -- Single-tenant initially, but the Wasm isolation model must be in place from Day 1. No retrofitting.
- [ ] **Web Chat UI (REQ-UI-001)** -- Built-in web server with streaming chat. The primary user interface.
- [ ] **Basic Permissions (REQ-HITL-001)** -- Simple tool approval: ask before file writes and network calls. The full 4-tier model can wait.
- [ ] **Session Persistence (REQ-MEMORY-003)** -- Save and resume sessions. File-based persistence for single-node mode.

### Add After Validation (v1.x) -- The Creator Experience

Features to add once the core agent loop works and the thesis is partially validated.

- [ ] **Interactive Skill Creation (REQ-SKILL-003)** -- This is the "aha moment." Add once the basic agent loop and skill format are stable. The built-in "skill writer" agent is the first practical demonstration of the product's value.
- [ ] **Wasm Sandbox Skill Preview (REQ-SKILL-003)** -- Preview mode for testing skills in isolation. Ships with interactive creation.
- [ ] **Hybrid Planning (REQ-AGENT-001 enhancement)** -- Add the plan-then-ReAct layer on top of the working ReAct loop. Users see "Step 2 of 5."
- [ ] **Skill Version Management (REQ-SKILL-004)** -- Git-backed version history for skills. Enables iteration and rollback.
- [ ] **Long-Term Memory (REQ-MEMORY-002)** -- Cross-session memory, initially with explicit save (user clicks "remember this"), later with background extraction.
- [ ] **Full HITL Policy Model (REQ-HITL-001)** -- The 4-tier policy: never/write-only/high-confidence/always. Builds on basic permissions.
- [ ] **Execution Intervention (REQ-HITL-002)** -- Pause, redirect, terminate mid-execution.

### Future Consideration (v2+) -- The Enterprise Platform

Features to defer until product-market fit is established with single-user creators.

- [ ] **Multi-Tenancy (REQ-SECURITY-001 full)** -- The Wasm isolation is already there from v1, but true multi-tenant routing, tenant provisioning, and data isolation across tenants lands here.
- [ ] **Personal-to-Enterprise Publish (REQ-DEPLOY-003)** -- The signature feature. Requires multi-tenancy, auth, audit, and skill packaging to all be solid.
- [ ] **Three-Layer Auth (REQ-SECURITY-005)** -- API Key, Session Token, Runtime Permission Check. Enterprise requirement.
- [ ] **Full Audit Logging (REQ-OBSERVABILITY-002)** -- Parameter-sanitized, searchable audit trail.
- [ ] **Cluster Mode (REQ-DEPLOY-002)** -- Multi-node with Redis + object storage for horizontal scaling.
- [ ] **Skill Composition DAG (REQ-SKILL-005)** -- Multi-skill workflows via the host message bus.
- [ ] **Multi-Agent Orchestration (REQ-AGENT-003)** -- Router-to-specialist with Wasm instance topology.
- [ ] **Skill Discovery/Market (REQ-UI-003)** -- Git-based skill discovery UI.
- [ ] **Scheduled/Triggered Execution (REQ-DEPLOY-004)** -- Cron and webhook triggers.
- [ ] **JIT Trace Caching** -- Hot path compilation for frequently-executed tool sequences.
- [ ] **Observability Dashboard (REQ-OBSERVABILITY-003 full)** -- Prometheus metrics, Grafana dashboards.

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Agent Loop (ReAct) | HIGH | MEDIUM | P1 |
| Tool Calling (MCP) | HIGH | MEDIUM | P1 |
| Streaming (SSE) | HIGH | LOW | P1 |
| Short-Term Memory | HIGH | MEDIUM | P1 |
| Skill Definition Format | HIGH | LOW | P1 |
| Skill Loading | HIGH | MEDIUM | P1 |
| Wasm Instance Isolation | HIGH | HIGH | P1 |
| Web Chat UI | HIGH | LOW | P1 |
| Session Persistence | MEDIUM | MEDIUM | P1 |
| Basic Permissions | MEDIUM | LOW | P1 |
| Interactive Skill Creation | VERY HIGH | HIGH | P2 |
| Wasm Sandbox Preview | HIGH | HIGH | P2 |
| Hybrid Planning | MEDIUM | HIGH | P2 |
| Skill Version Management | MEDIUM | LOW | P2 |
| Long-Term Memory | MEDIUM | MEDIUM | P2 |
| Full HITL Policy Model | MEDIUM | MEDIUM | P2 |
| Execution Intervention | MEDIUM | MEDIUM | P2 |
| Multi-Tenancy (full) | HIGH | HIGH | P3 |
| Personal-to-Enterprise Publish | VERY HIGH | VERY HIGH | P3 |
| Three-Layer Auth | MEDIUM | MEDIUM | P3 |
| Audit Logging | MEDIUM | MEDIUM | P3 |
| Cluster Mode | MEDIUM | HIGH | P3 |
| Skill Composition DAG | HIGH | HIGH | P3 |
| Multi-Agent Orchestration | HIGH | HIGH | P3 |
| Skill Discovery/Market | MEDIUM | LOW | P3 |
| Scheduled Execution | LOW | MEDIUM | P3 |
| JIT Trace Caching | LOW | VERY HIGH | P3 |
| Observability Dashboard | LOW | MEDIUM | P3 |

**Priority key:**
- P1: Must have for MVP launch
- P2: Add after core validation (v1.x)
- P3: Enterprise platform phase (v2+)

## Competitor Feature Analysis

| Feature | Claude Code | OpenAI GPTs / Agents SDK | LangGraph / Deep Agents | OpenClaw | jadepaw Approach |
|---------|-------------|--------------------------|------------------------|----------|-----------------|
| Agent Loop | ReAct (think-act-observe) | ReAct via Agents SDK | State graph with nodes | ReAct | ReAct MVP, Hybrid planning v1.1 |
| Skill/Plugin System | SKILL.md + Plugins with plugin.json manifest | GPT Builder conversation + Actions schema | Not a skill system; code-based graph definitions | skills/ directories | SKILL.md (Agent Skills standard) + interactive conversation creation |
| Multi-Agent | Subagents + Agent Teams (experimental) | Handoffs + Agent transfers | Supervisor + subgraph patterns | Single-agent personal assistant | Wasm-isolated router->specialist topology |
| Memory | Session transcripts (local files) | Thread-scoped + Vector store | Checkpointer + Store (namespaced long-term) | Session persistence | Short-term (checkpoint) + tenant-scoped long-term (pgvector/Qdrant) |
| Human-in-the-Loop | Permission modes (default/acceptEdits/plan/auto) + Hooks (PreToolUse, PermissionRequest) | `interrupt()` in SDK | `interrupt()` function + Command(resume=) | Chat-based approval | 4-tier policy model + execution pause/resume |
| Enterprise Multi-Tenancy | Managed settings (org-wide policies) | None (single-user) | None (framework, not platform) | None (personal assistant) | Wasm hardware isolation + 3-layer auth |
| Non-Developer UX | CLI/Terminal only | GPT Builder (conversational) + Chat | Developer-only (Python/JS) | CLI + messaging channels | Web Chat + Conversational Skill Builder + Hybrid Form Editor |
| Streaming | Real-time tool output | SSE streaming | Multi-mode (updates, messages, custom, debug) | Real-time messaging | SSE/WebSocket token streaming |
| Tool Protocol | MCP + built-in Bash/Read/Write/Edit/Glob/WebFetch/Grep | MCP + built-in tools | LangChain tools + custom | Channel adapters + custom tools | MCP-compatible + Wasm-mediated tool execution |
| Deployment Model | Local CLI + Agent SDK (library) | Cloud (chatgpt.com) + Agents SDK | Library/Framework (self-hosted) | Self-hosted Gateway | Built-in web server (local + remote unified UI) |

### Key Competitive Insights

1. **Claude Code is the closest analog** for the skill system. jadepaw adopts the SKILL.md format directly. The key gap Claude Code doesn't fill: no non-developer creation UX, no multi-tenant deployment, and no publishing path from personal to enterprise.

2. **OpenAI GPTs/Agents SDK** has the conversational creation UX that jadepaw aims for, but (a) it's cloud-only, (b) no Wasm sandbox preview, (c) no personal-to-enterprise publish path, (d) GPT Builder is being deprecated in favor of the Agents SDK which is developer-oriented.

3. **LangGraph/Deep Agents** is the most architecturally sophisticated agent framework, but it's purely for developers. The conceptual foundation (checkpointing, interrupts, Store, streaming modes) is what jadepaw should learn from for the underlying implementation, even though the target user is different.

4. **OpenClaw** is a personal assistant, not a platform. Its plugin architecture and channel abstraction are well-designed but aimed at a fundamentally different use case (single-user, personal devices).

5. **jadepaw's unique position**: The intersection of (a) Claude Code's skill system, (b) GPT Builder's conversational creation, (c) LangGraph's durable execution patterns, and (d) Wasm's hardware isolation -- all combined into a platform where non-developers create and deploy enterprise-grade agents.

## Sources

- **Claude Code Documentation (code.claude.com/docs)**: Agent loop, skills, subagents, hooks, permissions, plugins, agent teams, sessions, Agent SDK. Official Anthropic documentation. (HIGH confidence)
- **Agent Skills Open Standard (agentskills.io/specification)**: SKILL.md format specification, progressive disclosure model, directory structure. Open standard developed by Anthropic. (HIGH confidence)
- **LangGraph Documentation (docs.langchain.com)**: Agent concepts, multi-agent patterns, streaming, memory (short-term/long-term), interrupts (human-in-the-loop), durable execution. Official LangChain documentation. (HIGH confidence)
- **OpenClaw README (github.com/openclaw/openclaw)**: Personal AI assistant architecture, channel abstraction, plugin system. (MEDIUM confidence -- README is marketing, not deep docs)
- **jadepaw Architecture Doc (docs/jadepaw_discussion.md)**: Internal design decisions for Wasm runtime, instance pool, security model, JIT caching. (HIGH confidence -- primary source)
- **jadepaw Requirements (REQUIREMENTS.md)**: 28 requirements across 7 domains. (HIGH confidence -- primary source)
- **jadepaw MVP Decisions (.planning/notes/mvp-core-decisions.md)**: Product positioning, MVP scope, skill self-bootstrap design. (HIGH confidence -- primary source)

---
*Feature research for: jadepaw multi-tenant AI Agent runtime*
*Researched: 2026-05-28*