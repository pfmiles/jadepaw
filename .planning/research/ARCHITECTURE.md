# Architecture Research

**Domain:** Multi-tenant AI Agent Runtime Platform with WebAssembly Isolation
**Researched:** 2026-05-28
**Confidence:** MEDIUM

## Standard Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          INGRESS LAYER                                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                       │
│  │ TLS Term     │  │ Rate Limiter │  │ Auth (API    │                       │
│  │ (axum/tower) │  │              │  │ Key / JWT)   │                       │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                       │
│         └─────────────────┴─────────────────┘                               │
├─────────────────────────────────────────────────────────────────────────────┤
│                       GATEWAY / ROUTING LAYER                                │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │  Session Affinity Router (SessionID → Node + Instance mapping)       │   │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────────┐             │   │
│  │  │ WebSocket  │  │ HTTP/SSE   │  │ Connect → Session  │             │   │
│  │  │ Upgrade    │  │ Streaming  │  │ Mapping Registry   │             │   │
│  │  └────────────┘  └────────────┘  └────────────────────┘             │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                       CORE RUNTIME LAYER                                     │
│  ┌──────────────────────────────────┐  ┌──────────────────────────────┐     │
│  │    Agent Orchestrator            │  │    Instance Pool Manager     │     │
│  │  ┌──────────┐ ┌──────────────┐   │  │  ┌──────────┐ ┌──────────┐  │     │
│  │  │ Planner  │ │ ReAct        │   │  │  │ Pooling  │ │ InstPre  │  │     │
│  │  │ (LLM)    │ │ Executor     │   │  │  │ Alloc    │ │ Cache    │  │     │
│  │  └──────────┘ └──────────────┘   │  │  └──────────┘ └──────────┘  │     │
│  │  ┌──────────┐ ┌──────────────┐   │  │  ┌──────────┐ ┌──────────┐  │     │
│  │  │ Tool     │ │ Memory       │   │  │  │ State     │ │ Resource │  │     │
│  │  │ Registry │ │ Manager      │   │  │  │ Injector  │ │ Limiter  │  │     │
│  │  └──────────┘ └──────────────┘   │  │  └──────────┘ └──────────┘  │     │
│  └──────────────────────────────────┘  └──────────────────────────────┘     │
├─────────────────────────────────────────────────────────────────────────────┤
│                       SANDBOX EXECUTION LAYER                                │
│  ┌──────────────────────┐  ┌─────────────────────┐  ┌──────────────────┐    │
│  │  Tool Sandbox        │  │  Wasm Instance Cage  │  │  Network Guard   │    │
│  │  ┌────────────────┐  │  │  ┌───────────────┐   │  │  ┌────────────┐  │    │
│  │  │ user namespace │  │  │  │ Independent   │   │  │  │ Domain     │  │    │
│  │  │ chroot         │  │  │  │ Memory/Table  │   │  │  │ Allowlist  │  │    │
│  │  │ seccomp filter │  │  │  │ per instance  │   │  │  │ SSRF Guard │  │    │
│  │  └────────────────┘  │  │  └───────────────┘   │  │  └────────────┘  │    │
│  └──────────────────────┘  └─────────────────────┘  └──────────────────┘    │
├─────────────────────────────────────────────────────────────────────────────┤
│                       MESSAGE BUS LAYER                                      │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │  tokio::broadcast (intra-node) + NATS/Redis PubSub (inter-node)      │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────────────┐    │   │
│  │  │ Agent→   │  │ Tool→    │  │ System   │  │ Skill DAG        │    │   │
│  │  │ Agent    │  │ Agent    │  │ Events   │  │ Orchestration    │    │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────────────┘    │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────────┤
│                       DATA LAYER                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│  │  Redis       │  │  PostgreSQL  │  │  MinIO/S3    │  │  etcd        │    │
│  │  Session     │  │  Tenant      │  │  Tool Output │  │  Config      │    │
│  │  State       │  │  Data        │  │  Blobs       │  │  Cluster     │    │
│  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Typical Implementation |
|-----------|----------------|------------------------|
| TLS Terminator | HTTPS termination, certificate management | axum + tower-http + rustls |
| Auth Middleware | API Key / JWT validation, tenant identity extraction | tower Layer (axum middleware) |
| Session Affinity Router | Maps SessionID to host node + Wasm instance, routes WebSocket/SSE streams | DashMap<SessionId, RouteTarget> + axum WebSocket upgrade |
| Agent Orchestrator | Hybrid loop: plan generation (LLM) + ReAct step execution (think/tool/observe) | tokio task per session, state machine over loop phases |
| Instance Pool Manager | Pre-allocated Wasm instance pool, state injection on session bind, reset on release | PoolingInstanceAllocator + InstancePre + custom pool layer |
| State Injector | Writes session ID, tenant config, capability set into Store data before execution | Store::data_mut() + custom state struct |
| Resource Limiter | Enforces per-instance memory, table, instance count, and fuel limits | wasmtime::ResourceLimiter + StoreLimits |
| Tool Registry | Catalog of allowed tools per tenant/session, MCP-compatible protocol | HashMap<ToolId, ToolDefinition> per tenant scope |
| Memory Manager | Short-term (window compression) + long-term (vector DB) memory, per-tenant isolation | Redis for short-term, pgvector for long-term |
| Tool Sandbox | Spawns external tool processes in user namespace + chroot + seccomp | clone() with CLONE_NEWUSER + pivot_root + seccomp BPF |
| Wasm Instance Cage | Per-instance independent Memory and Table objects, preopens directory | wasmtime::Memory, preopened_dir per session |
| Network Guard | Allowlist domain check on host_http_fetch, SSRF prevention | Domain pattern matching + private IP range rejection |
| Message Bus (intra-node) | tokio::broadcast for agent-to-agent, tool-to-agent, system events | Channel per event type, sharded by tenant |
| Message Bus (inter-node) | NATS/Redis PubSub for cross-node agent communication | NATS subject-based routing, Redis PubSub channels |
| Redis (Session State) | Ephemeral session context, conversation history, ReAct loop state | redis-rs async cluster client |
| PostgreSQL (Tenant Data) | Persistent tenant config, skill definitions, user accounts, audit trail | sqlx or diesel async |
| MinIO/S3 (Blobs) | Tool outputs, file artifacts, skill package storage | aws-sdk-rust or minio client |

## Recommended Project Structure

```
crates/
├── jadepaw-core/              # Core data types, traits, error types shared across all crates
│   ├── src/
│   │   ├── types.rs           # SessionId, TenantId, ToolId, SkillId, CapabilitySet
│   │   ├── error.rs           # Unified error types
│   │   ├── config.rs          # Config structs (global/tenant/session layers)
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-wasm/              # Wasm runtime: instance pool, engine management, host functions
│   ├── src/
│   │   ├── engine.rs          # wasmtime::Engine setup, Config, compilation cache
│   │   ├── pool.rs            # InstancePool: pre-warm, acquire, release, reset
│   │   ├── store.rs           # Custom Store data: session context, capabilities, limiter
│   │   ├── linker.rs          # Host function registration (read_file, http_fetch, etc.)
│   │   ├── host/              # Host function implementations
│   │   │   ├── filesystem.rs  # Preopens, path validation, read/write with sandbox roots
│   │   │   ├── network.rs     # Domain allowlist, SSRF prevention, HTTP fetch
│   │   │   └── logging.rs     # Structured log emission to audit pipeline
│   │   ├── limits.rs          # ResourceLimiter impl: memory, table, instance caps
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-agent/             # Agent runtime: hybrid loop, planning, ReAct execution
│   ├── src/
│   │   ├── orchestrator.rs    # Top-level session orchestrator, manages loop lifecycle
│   │   ├── planner.rs         # LLM-driven plan generation, re-planning on deviation
│   │   ├── react.rs           # ReAct step executor: think -> tool -> observe -> next
│   │   ├── tool/              # Tool system
│   │   │   ├── registry.rs    # Tool registration, MCP protocol adapter
│   │   │   ├── sandbox.rs     # Spawn tool in user namespace + chroot + seccomp
│   │   │   └── approval.rs    # Human-in-the-loop approval gate for high-risk tools
│   │   ├── memory.rs          # Short-term (window compress) and long-term (vector) memory
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-skill/             # Skill system: creation, compilation, publishing
│   ├── src/
│   │   ├── format.rs          # Declarative skill format (Markdown/YAML structured skeleton)
│   │   ├── creator.rs         # Interactive skill creation via LLM dialogue
│   │   ├── compiler.rs        # Natural language skill → Wasm bytecode compilation
│   │   ├── registry.rs        # Skill catalog, versioning, git-based distribution
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-gateway/           # HTTP gateway: routing, WebSocket, session affinity
│   ├── src/
│   │   ├── router.rs          # SessionID extraction, node/instance routing
│   │   ├── ws.rs              # WebSocket upgrade, bidirectional streaming
│   │   ├── auth.rs            # API Key/JWT middleware, tenant identity extraction
│   │   ├── session.rs         # Session registry (DashMap<SessionId, SessionContext>)
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-bus/               # Message bus: intra-node and inter-node event routing
│   ├── src/
│   │   ├── local.rs           # tokio::broadcast per event namespace, sharded by tenant
│   │   ├── remote.rs          # NATS/Redis PubSub bridge for cross-node communication
│   │   ├── events.rs          # Event type definitions (AgentEvent, ToolEvent, SystemEvent)
│   │   └── lib.rs
│   └── Cargo.toml
├── jadepaw-server/            # Binary crate: wires everything together
│   ├── src/
│   │   ├── main.rs            # Startup, config loading, crate initialization order
│   │   ├── app.rs             # axum Router assembly, middleware stack
│   │   └── shutdown.rs        # Graceful shutdown, pool drain, session migration
│   └── Cargo.toml
└── jadepaw-frontend/          # Web UI (separate from Rust crates)
    ├── src/
    │   ├── chat/              # Streaming Web Chat component
    │   ├── skills/            # Skill creation, preview, management UI
    │   └── market/            # Skill discovery and install
    └── package.json
```

### Structure Rationale

- **jadepaw-core**: Shared types prevent circular dependencies. Everything depends on core, core depends on nothing internal.
- **jadepaw-wasm**: Clean separation of Wasm runtime concerns. Can be tested independently with mock agent workloads. Ownership of all unsafe code is concentrated here.
- **jadepaw-agent**: Agent loop logic separated from Wasm mechanics. The orchestrator receives an already-instantiated Wasm Store from the pool layer -- it doesn't know about pools or allocation.
- **jadepaw-skill**: Skill compilation is isolated from the runtime that executes skills. This allows the compiler to use larger Wasm modules or different compilation strategies without affecting runtime performance.
- **jadepaw-gateway**: Gateway is the only crate that knows about HTTP. Agent runtime doesn't import axum or hyper.
- **jadepaw-bus**: Event bus is its own crate so both gateway and agent can publish/consume events without depending on each other.
- **jadepaw-server**: Thin binary crate. Only responsibility is dependency injection (wiring crates together) and config loading. Business logic lives in library crates.

## Architectural Patterns

### Pattern 1: Pre-Warmed Instance Pool with State Injection

**What:** Instead of creating a new Wasm Store+Instance per session request, the pool manager pre-instantiates N "blank" instances at startup. Each blank instance has the agent base module loaded and host functions linked, but no session state. When a session begins, an instance is pulled from the pool, session-specific state (tenant ID, capabilities, tool whitelist) is injected into the Store's data, and execution begins. On session end, the Store data is reset to blank, and the instance returns to the pool.

**When to use:** When cold start time must be under 5ms and the system supports 10,000+ concurrent instances on a single machine. This is the core performance pattern for the entire platform.

**Trade-offs:** Pool pre-allocation consumes memory upfront even when idle. Instance reuse means careful reset discipline -- any leaked state from a previous session is a multi-tenant isolation violation. The pool must handle fragmentation (instances with different module versions) gracefully.

**Example (Rust/wasmtime):**
```rust
struct InstancePool {
    config: PoolConfig,
    engine: Engine,
    linker: Arc<Linker<SessionState>>,
    blank_module: Module,
    available: Arc<Semaphore>,
    // Group instances by module version for zero-downtime upgrades
    pools: DashMap<ModuleVersion, crossbeam_queue::ArrayQueue<InstancePre<SessionState>>>,
}

impl InstancePool {
    /// Startup: pre-instantiate `warm_count` instances
    async fn warm(&self, version: ModuleVersion) -> Result<()> {
        let pre = self.linker.instantiate_pre(&self.blank_module)?;
        let queue = ArrayQueue::new(self.config.max_per_version);
        for _ in 0..self.config.warm_count {
            let store = Store::new(&self.engine, SessionState::blank());
            store.limiter(|s| &mut s.limits);
            let inst = pre.instantiate_async(&mut store).await?;
            queue.push((store, inst)).map_err(|_| PoolError::QueueFull)?;
        }
        self.pools.insert(version, queue);
        Ok(())
    }

    /// Acquire: pop from pool, inject session state
    async fn acquire(&self, session: SessionContext) -> Result<ActiveInstance> {
        let (mut store, instance) = self.pools[&session.module_version]
            .pop()
            .ok_or(PoolError::Exhausted)?;
        // Inject session-specific state into Store data
        *store.data_mut() = SessionState {
            session_id: session.id,
            tenant_id: session.tenant_id,
            capabilities: session.capabilities,
            limits: StoreLimitsBuilder::new()
                .memory_size(session.memory_limit_mb as usize * 1024 * 1024)
                .instances(2)
                .build(),
            ..Default::default()
        };
        Ok(ActiveInstance { store, instance, session_id: session.id })
    }
}
```

### Pattern 2: Hybrid Agent Loop (Plan-ReAct)

**What:** The agent execution is a two-phase loop. Phase 1 (Planning): the LLM generates a coarse-grained plan of 3-7 high-level steps. Phase 2 (Execution): each step is executed via a ReAct loop (think -> tool call -> observe result -> decide next action). If the ReAct loop detects that execution has deviated from the plan, it triggers a local re-plan for the remaining steps rather than restarting the entire task.

**When to use:** When the agent needs both goal-oriented coherence (the plan keeps it on track) and execution flexibility (ReAct adapts to real-time observations). This is the design documented in REQ-AGENT-001.

**Trade-offs:** Two invocations of the LLM per major decision (plan generation + ReAct thinking). More token consumption than pure ReAct. But produces more deliberate, auditable behavior -- the plan is inspectable before execution begins. The plan also serves as a progress indicator for the user ("Step 2/5: aggregating data from Slack").

**Example (state machine):**
```rust
enum LoopPhase {
    Planning,                        // Generate initial plan
    Executing { step: usize },       // ReAct loop on current step
    RePlanning { from_step: usize }, // Deviation detected, re-plan remaining steps
    Complete,                        // All steps done
    Failed { reason: String },       // Unrecoverable error
}

struct AgentLoop {
    phase: LoopPhase,
    plan: Vec<PlanStep>,
    session: SessionContext,
    llm: LlmClient,
    tool_registry: Arc<ToolRegistry>,
}

impl AgentLoop {
    async fn tick(&mut self) -> Result<AgentEvent> {
        match self.phase {
            LoopPhase::Planning => {
                self.plan = self.llm.generate_plan(&self.session.context).await?;
                self.phase = LoopPhase::Executing { step: 0 };
                Ok(AgentEvent::PlanGenerated(self.plan.clone()))
            }
            LoopPhase::Executing { step } => {
                let result = self.react_step(&self.plan[step]).await?;
                match result {
                    StepResult::Complete => {
                        let next = step + 1;
                        if next >= self.plan.len() {
                            self.phase = LoopPhase::Complete;
                        } else {
                            self.phase = LoopPhase::Executing { step: next };
                        }
                    }
                    StepResult::Deviated(deviation) => {
                        self.phase = LoopPhase::RePlanning { from_step: step };
                    }
                }
                Ok(AgentEvent::StepProgress { step, result })
            }
            LoopPhase::RePlanning { from_step } => {
                let new_tail = self.llm.replan(&self.plan[..from_step], &self.session).await?;
                self.plan.truncate(from_step);
                self.plan.extend(new_tail);
                self.phase = LoopPhase::Executing { step: from_step };
                Ok(AgentEvent::PlanRevised(self.plan.clone()))
            }
            LoopPhase::Complete => Ok(AgentEvent::Done),
            LoopPhase::Failed { .. } => Ok(AgentEvent::Error),
        }
    }
}
```

### Pattern 3: Capability-Based Security (Default-Deny)

**What:** Every Wasm instance starts with zero capabilities. The tenant configuration explicitly whitelists which host functions, filesystem paths, network domains, and tools the instance can access. This whitelist is injected into the Store data at session bind time. Every host function call checks the capability set before executing.

**When to use:** Multi-tenant environments where different tenants have different trust levels and access requirements. This is non-negotiable for the jadepaw platform per REQ-SECURITY-003.

**Trade-offs:** Capability checks add a small per-host-function-call overhead. Must be careful to check capabilities at the earliest possible point in each host function to avoid partial side effects before denial. The capability model must be serializable (for cross-node session migration).

**Example:**
```rust
struct CapabilitySet {
    file_read: Vec<PathPattern>,      // e.g., "/data/tenant-{id}/**"
    file_write: Vec<PathPattern>,
    network_domains: Vec<DomainPattern>, // e.g., "api.slack.com", "*.github.com"
    allowed_tools: HashSet<ToolId>,
    max_memory_mb: u32,
    max_execution_seconds: u64,
}

impl SessionState {
    fn can_read_file(&self, path: &Path) -> bool {
        let normalized = normalize_path(path);
        self.capabilities.file_read.iter().any(|pattern| pattern.matches(&normalized))
    }

    fn can_call_tool(&self, tool_id: ToolId) -> bool {
        self.capabilities.allowed_tools.contains(&tool_id)
    }

    fn can_access_domain(&self, domain: &str) -> bool {
        // Also check for private/loopback IP ranges (SSRF prevention)
        if is_private_or_loopback(domain) {
            return false;
        }
        self.capabilities.network_domains.iter().any(|pattern| pattern.matches(domain))
    }
}
```

### Pattern 4: Event-Driven Communication via Sharded Broadcast

**What:** All communication between agents, between tools and agents, and for system announcements flows through a typed event bus. Within a single node, `tokio::sync::broadcast` channels are sharded by tenant ID to prevent tenant-A's events from being visible to tenant-B. Cross-node communication uses NATS subject-based addressing with tenant-scoped subjects. The message bus crate provides a unified `MessageBus` trait that abstracts over local and remote transports.

**When to use:** When the system has multiple communicating components (agent orchestrator, tool sandbox, memory manager, skill compiler) that need loose coupling. Also when Skill DAGs need to pass messages between skill nodes.

**Trade-offs:** Event-driven architectures add indirection -- harder to trace a request end-to-end than direct function calls. But for a multi-agent system where agents can spawn sub-agents (REQ-AGENT-003) and skills compose into DAGs (REQ-SKILL-005), events are the only scalable communication pattern.

**Example:**
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
enum AgentEvent {
    PlanGenerated { session_id: SessionId, plan: Vec<PlanStep> },
    StepProgress { session_id: SessionId, step: usize, result: StepResult },
    ToolCallRequest { session_id: SessionId, tool_id: ToolId, params: Value },
    ToolCallResult { session_id: SessionId, tool_id: ToolId, output: Value },
    SubAgentSpawned { parent_session: SessionId, child_session: SessionId },
    SkillDagMessage { dag_id: SkillDagId, from_node: String, to_node: String, payload: Value },
    PlanRevised { session_id: SessionId, plan: Vec<PlanStep> },
    SessionError { session_id: SessionId, error: String },
    SessionComplete { session_id: SessionId },
}

struct ShardedBus {
    // One broadcast channel per tenant shard
    shards: DashMap<String, broadcast::Sender<AgentEvent>>,
    // NATS client for cross-node relay
    bridge: Option<async_nats::Client>,
    shard_count: usize,
}

impl ShardedBus {
    fn publish(&self, tenant_id: &TenantId, event: AgentEvent) -> Result<()> {
        let shard_key = tenant_shard(tenant_id, self.shard_count);
        if let Some(tx) = self.shards.get(&shard_key) {
            let _ = tx.send(event.clone());
        }
        // Cross-node relay: publish to NATS subject scoped by tenant
        if let Some(ref nats) = self.bridge {
            let subject = format!("jadepaw.tenants.{}.events", tenant_id);
            let payload = serde_json::to_vec(&event)?;
            tokio::spawn(async move {
                let _ = nats.publish(subject, payload.into()).await;
            });
        }
        Ok(())
    }
}
```

## Data Flow

### Session Lifecycle Flow

```
[User WebSocket Connect]
    │
    ▼
[Gateway: Extract Tenant JWT, Validate Session Token]
    │
    ├── New session? ──► [Gateway: Create SessionId, Store in Redis]
    │                         │
    ├── Existing session? ──► [Gateway: Look up node affinity]
    │                         │
    ├── Wrong node? ──► [Gateway: Proxy to correct node (NATS lookup)]
    │
    ▼
[Gateway: Acquire Wasm Instance from Pool]
    │
    ▼
[State Injector: Write tenant config, capabilities, session context into Store]
    │
    ▼
[Agent Orchestrator: Kick off Hybrid Loop]
    │
    ▼
┌─────────────────────────────────────────────────────────┐
│  Loop Phase: Planning                                    │
│  [LLM Client] ─► Generate plan ─► Publish PlanGenerated  │
└───────────────────────┬─────────────────────────────────┘
                        ▼
┌─────────────────────────────────────────────────────────┐
│  Loop Phase: Executing (ReAct)                           │
│  ┌──────────────────────────────────────────────────┐   │
│  │  Think ─► Tool Call ─► Host Fn ─► Cap Check      │   │
│  │    ▲                        │                    │   │
│  │    │                        ▼                    │   │
│  │    └──── Observe ◄── Tool Sandbox (if external)  │   │
│  └──────────────────────────────────────────────────┘   │
│  On deviation: trigger RePlanning                        │
│  Each step: stream progress to WebSocket                 │
└───────────────────────┬─────────────────────────────────┘
                        ▼
[Session End: Reset Store data, return instance to pool]
    │
    ▼
[Persist session summary to Redis/PostgreSQL]
```

### Request Flow (Detailed)

```
HTTP Request (POST /api/sessions/{id}/message)
    │
    ▼
[TLS Terminator] ─► [Auth Middleware: validate JWT, extract tenant_id]
    │
    ▼
[Session Router: look up SessionId → (node_id, instance_id) in DashMap]
    │
    ├── Local? ─► [Acquire instance handle from pool]
    │
    ├── Remote? ─► [Proxy via internal gRPC/NATS to target node]
    │
    ▼
[Agent Orchestrator: push message into ReAct loop]
    │
    ▼
[ReAct: Think → Tool Call → Observe]
    │
    ├── Internal tool? ─► [Host function call with capability check]
    │
    ├── External tool? ─► [Tool Sandbox: fork → user namespace → chroot → execute → capture output]
    │
    ▼
[Stream response tokens via WebSocket/SSE to client]
```

### State Management

```
┌──────────────────────────────────────────────────────┐
│                   State Layers                        │
├──────────────────────────────────────────────────────┤
│  Ephemeral (Wasm Store data, lifetime = session)      │
│  - Session context, plan state, ReAct loop position   │
│  - Held in Store::data_mut(), lost on instance reset  │
├──────────────────────────────────────────────────────┤
│  Session State (Redis, lifetime = session)            │
│  - Conversation history, compressed window            │
│  - Checkpoint for cross-node migration                │
│  - Key: session:{id}:history                          │
├──────────────────────────────────────────────────────┤
│  Tenant State (PostgreSQL, lifetime = tenant)         │
│  - Tenant config, skill definitions, user accounts    │
│  - Capability sets, tool registrations                │
│  - Key: tenant_id primary key                         │
├──────────────────────────────────────────────────────┤
│  Global State (etcd, lifetime = cluster)              │
│  - Node registry, active session map                  │
│  - Feature flags, global rate limits                  │
│  - Key: /jadepaw/nodes/{node_id}                      │
└──────────────────────────────────────────────────────┘
```

### Key Data Flows

1. **Session Creation:** Gateway creates SessionId -> stores in Redis -> acquires Wasm instance -> injects state -> starts agent loop -> returns session handle to client.
2. **Agent Execution:** LLM generates plan (stored in Store data) -> each ReAct step calls tools via message bus -> tool results observed -> plan updated -> streaming output to WebSocket.
3. **Tool Execution:** Agent publishes ToolCallRequest event -> tool sandbox receives -> validates capabilities -> spawns subprocess in user namespace -> publishes ToolCallResult event -> agent observes.
4. **Session Migration:** Node failure detected -> session state read from Redis -> new node acquires fresh Wasm instance -> replays conversation history into context -> resumes agent loop from checkpoint.
5. **Skill Compilation:** User describes skill in natural language -> LLM generates structured skill skeleton -> skill compiler produces Wasm module -> loaded into sandbox for preview -> user iterates -> publish triggers packaging and distribution.

## Scaling Considerations

| Scale | Architecture Adjustments |
|-------|--------------------------|
| 0-1k sessions (single node) | Everything in one process. Redis optional (can use in-memory DashMap). No NATS needed -- tokio::broadcast is sufficient. Object storage can be local filesystem. |
| 1k-100k sessions (small cluster) | Redis cluster for session state. NATS for cross-node event bus. PostgreSQL with connection pooling (deadpool). Instance pools per node. Nginx/Envoy for TLS termination and load balancing. Sticky sessions (cookie-based) for WebSocket affinity. |
| 100k+ sessions (large cluster) | Multi-region deployment. Redis Geo-replication. NATS supercluster with gateways. PostgreSQL read replicas + Citus sharding. etcd cluster for node discovery. Session migration between regions for latency optimization. Instance pool warming based on predictive load. |

### Scaling Priorities

1. **First bottleneck (1k-10k sessions):** Wasm instance pool exhaustion. Fix: increase pool size, add backpressure (reject new sessions with 503 when pool below threshold), implement instance eviction for idle sessions.
2. **Second bottleneck (10k-100k sessions):** Single Redis instance for session state. Fix: Redis cluster with consistent hashing by SessionId, session state TTLs, checkpoint compression.
3. **Third bottleneck (100k+):** Cross-node event bus saturation. Fix: NATS cluster with leaf nodes, event batching, priority-based event queue (system events > tool results > progress updates).

## Anti-Patterns

### Anti-Pattern 1: Global Mutable Store Sharing

**What people do:** Share a single wasmtime::Store across multiple sessions or tenants, relying on Store data partitioning to provide isolation.

**Why it's wrong:** A Store in wasmtime owns all Wasm linear memory and tables. Sharing it between tenants means a memory corruption bug in one tenant's Wasm code can leak data to another tenant. The wasmtime documentation explicitly ties Store to single-tenant execution contexts.

**Do this instead:** One Store per active session, strictly. The pooling allocator shares the underlying memory pool (for allocation efficiency), but each Store gets its own Memory and Table objects from the pool. When a session ends, the Store is dropped, returning its resources to the pool and guaranteeing zero data residue.

### Anti-Pattern 2: Path Concatenation Without Normalization

**What people do:** Join a sandbox root path with a user-provided filename using standard path concatenation: `sandbox_root.join(user_path)`.

**Why it's wrong:** `user_path` may contain `../` or symbolic links that escape the sandbox root. Path traversal is one of the most common security vulnerabilities in sandboxed execution environments.

**Do this instead:** Normalize the path (resolve `.`, `..`, symlinks), construct the full path, and verify that the result is still a child of the sandbox root. Never trust user-provided paths even after normalization -- always do the starts_with check. See the `validate_path` function in the project design document for the canonical implementation.

### Anti-Pattern 3: LLM Directly in Wasm Instance

**What people do:** Compile the LLM inference client into the Wasm module that runs in the sandbox, so the agent can call the LLM without host function round-trips.

**Why it's wrong:** LLM API keys would live inside Wasm linear memory, where they could be exfiltrated by a compromised module. LLM inference is also a high-latency, high-resource operation -- running it inside Wasm defeats the purpose of lightweight instance management.

**Do this instead:** The LLM client lives in the host (jadepaw-agent crate). The Wasm module can request LLM inference via a host function, but the actual API call, authentication, rate limiting, and response parsing happen on the host side. The Wasm module only sees the prompt it requested and the response it received.

### Anti-Pattern 4: Coupling Agent Loop to Transport

**What people do:** Embed WebSocket frame management inside the agent orchestrator, tightly coupling the agent loop to the transport protocol.

**Why it's wrong:** The agent loop should be transport-agnostic. Session migration between nodes requires the loop to be restartable from a checkpoint. Triggered execution (cron/webhook per REQ-DEPLOY-004) has no WebSocket connection at all.

**Do this instead:** The agent orchestrator emits AgentEvent values into the message bus. A separate transport adapter (in jadepaw-gateway) subscribes to events for active WebSocket sessions and serializes them to wire format. For triggered execution, the event bus routes output to a notification channel. The agent loop never knows what transport is being used.

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| LLM API (Anthropic/OpenAI) | HTTP client in jadepaw-agent, not in Wasm | API key lives in host config, never in Wasm memory. Rate limiting and retry in host. |
| Redis | redis-rs async with deadpool connection pool | Session state keys: `session:{id}:{field}`. Use RESP3 protocol for push-based pub/sub. |
| PostgreSQL | sqlx async with connection pool | Tenant data, skill registry, audit logs. Use row-level security for multi-tenant queries as defense-in-depth. |
| NATS | async-nats client | Subject namespace: `jadepaw.{node_id}.{function}`. JetStream for persistent event streams if needed for audit. |
| MinIO/S3 | aws-sdk-s3 or minio Rust client | Tool outputs, skill packages. Bucket per tenant, presigned URLs for direct upload/download. |

### Internal Boundaries

| Boundary | Communication | Notes |
|----------|---------------|-------|
| Gateway ↔ Agent Orchestrator | Direct function call (same process) or gRPC (cross-node) | For same-node: acquire instance handle, push message. Cross-node: serialize via protobuf. |
| Agent Orchestrator ↔ Wasm Pool | Direct function call (always same process) | Acquire/release instance. Pool knows about active sessions for fair scheduling. |
| Agent Orchestrator ↔ Tool Sandbox | Message bus (ToolCallRequest/ToolCallResult events) | Decoupled so tool execution can be on a different thread or node. Timeout via tokio::time::timeout. |
| Wasm Host Functions ↔ Host System | Direct function call within Store context | Capability check at every entry point. Path validation before filesystem access. |
| Skill Compiler ↔ Wasm Engine | Direct API call | Compiler produces .wasm bytes, engine loads via Module::from_binary. Preview in sandbox before publish. |
| Message Bus (local) ↔ Message Bus (remote) | Bridge adapter | Local events are broadcast via tokio::broadcast. Remote adapter subscribes and forwards to NATS. Remote events arrive via NATS and are injected into local broadcast. |

## Build Order (Dependency Graph)

```
Phase 1: Foundation
──────────────────
  jadepaw-core  (types, errors, config)
       │
  jadepaw-wasm  (engine, pool, host functions, limits)
       │
  jadepaw-gateway (basic HTTP, auth, session registration)
       │
  jadepaw-server (wire core+wasm+gateway, single-node MVP)

Phase 2: Agent Intelligence
──────────────────────────
  jadepaw-bus   (local event broadcast)
       │
  jadepaw-agent (orchestrator, planner, ReAct, tools, memory)
       │
  jadepaw-skill (format, creator, compiler)
       │
  jadepaw-gateway (add WebSocket streaming, session affinity)

Phase 3: Multi-Tenant Production
────────────────────────────────
  jadepaw-bus   (add NATS bridge)
       │
  jadepaw-agent (add multi-tenant isolation, approval gates)
       │
  jadepaw-skill (add publishing, marketplace)
       │
  jadepaw-server (cluster mode, Redis + NATS + etcd integration)

Phase 4: Scale & Polish
───────────────────────
  jadepaw-wasm  (JIT固化引擎, InstancePre cache optimization)
  jadepaw-bus   (JetStream persistence, event replay)
  jadepaw-frontend (Web Chat, Skill manager, Market UI)
```

### Build Order Rationale

The core dependency chain is: **jadepaw-core -> jadepaw-wasm -> jadepaw-bus -> jadepaw-agent**. The gateway depends on core + wasm (for session management) and bus (for event streaming). The skill crate depends on agent (for preview execution) and wasm (for compilation targets). The server crate sits at the top, wiring everything together.

Phase 1 should deliver a working Wasm instance pool with a basic HTTP endpoint that can load a module, inject state, and execute a host function call -- proving the core isolation and performance claims. No LLM needed yet. Phase 2 adds the agent loop and skill system. Phase 3 hardens for multi-tenancy. Phase 4 is optimization and UI.

## Sources

- wasmtime PoolingInstanceAllocator and InstancePre docs -- Context7 (`/bytecodealliance/wasmtime`, `/websites/rs_wasmtime`). HIGH confidence.
- wasmtime ResourceLimiter trait and Store::limiter configuration -- Context7 (`/websites/rs_wasmtime`). HIGH confidence.
- wasmtime epoch-based interruption for cooperative timeslicing -- Context7 (`/websites/rs_wasmtime`). HIGH confidence.
- wasmtime-wasi preopened_dir for filesystem sandboxing -- Context7 (`/websites/rs_wasmtime-wasi`). HIGH confidence.
- tokio mpsc and broadcast channels for intra-node message passing -- Context7 (`/websites/rs_tokio`). HIGH confidence.
- axum State, WebSocket upgrade, middleware, and routing -- Context7 (`/websites/rs_axum`). HIGH confidence.
- DashMap concurrent hashmap for session registry -- Context7 (`/xacrimon/dashmap`, `/websites/rs_dashmap`). HIGH confidence.
- NATS subject-based messaging and JetStream -- Context7 (`/websites/nats_io`). HIGH confidence.
- redis-rs async cluster client with RESP3 pub/sub -- Context7 (`/websites/rs_redis`). HIGH confidence.
- jadepaw project design document (docs/jadepaw_discussion.md) -- sections 3, 4, 5 for system architecture. HIGH confidence.
- jadepaw MVP core decisions (.planning/notes/mvp-core-decisions.md) -- Agent Loop design. HIGH confidence.
- jadepaw requirements (.planning/PROJECT.md) -- 28 requirements across 7 domains. HIGH confidence.
- Agent runtime patterns (ReAct, planning, state machines): synthesized from project requirements and LLM training data on LangGraph/OpenAI Agents patterns. MEDIUM confidence (literature not directly fetched, but patterns are broadly documented in public agent framework source code).

---
*Architecture research for: multi-tenant AI Agent runtime platform with WebAssembly isolation*
*Researched: 2026-05-28*