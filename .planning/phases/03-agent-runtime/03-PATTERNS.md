# Phase 03: Agent Runtime - Pattern Map

**Mapped:** 2026-06-01
**Files analyzed:** 10 (new + modified)
**Analogs found:** 9 / 10

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/jadepaw-core/src/agent_types.rs` | model | request-response | `crates/jadepaw-core/src/capabilities.rs` | role-match (data structs with serde) |
| `crates/jadepaw-core/src/guest_exports.rs` | trait-definition | request-response | `crates/jadepaw-core/src/host_functions.rs` | exact (additive-only trait pattern) |
| `crates/jadepaw-agent/src/loop.rs` | service | event-driven | `crates/jadepaw-wasm/src/pool.rs` | role-match (async orchestrator) |
| `crates/jadepaw-agent/src/llm.rs` | service | streaming | no direct analog | no-match (new domain -- see RESEARCH.md) |
| `crates/jadepaw-agent/src/guard.rs` | utility | event-driven | `crates/jadepaw-wasm/src/limits/instance_hard.rs` | role-match (policy boundary, tokio::select!) |
| `crates/jadepaw-agent/src/stream.rs` | utility | streaming | no direct analog | no-match (new domain -- see RESEARCH.md) |
| `crates/jadepaw-agent/src/lib.rs` (modify) | facade | request-response | `crates/jadepaw-wasm/src/lib.rs` | exact (crate root with re-exports) |
| `crates/jadepaw-core/src/error.rs` (modify) | model | request-response | existing file (self) | exact (extend existing enum) |
| `crates/jadepaw-core/src/lib.rs` (modify) | facade | request-response | existing file (self) | exact (extend existing mods) |
| `Cargo.toml` (workspace, modify) | config | config | existing file (self) | exact (extend existing dep) |

## Pattern Assignments

### `crates/jadepaw-core/src/agent_types.rs` (model, request-response)

**Analog:** `crates/jadepaw-core/src/capabilities.rs`

Pure data structures with serde Serialize/Deserialize, no wasmtime dependency. Lives in `jadepaw-core` so jadepaw-agent and other crates can depend on types without pulling in wasmtime.

**Imports pattern** (lines 1-8 of `capabilities.rs`):
```rust
use crate::types::ToolId;
use serde::{Deserialize, Serialize};
use std::fmt;
```

**Core pattern** (lines 58-72 of `capabilities.rs` -- struct definition with doc comments):
```rust
/// Doc comment describing the struct purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceCapabilities {
    /// Field-level doc comments.
    pub can_read_files: Vec<PathPattern>,
    /// ...
    pub max_memory_mb: u32,
    pub max_compute_units: u64,
}
```

**Default impl pattern** (lines 74-87 of `capabilities.rs`):
```rust
impl Default for InstanceCapabilities {
    fn default() -> Self {
        Self {
            can_read_files: Vec::new(),
            can_write_files: Vec::new(),
            // ...
            max_memory_mb: 64,
            max_compute_units: 0,
        }
    }
}
```

**Newtype wrapper pattern** (from `capabilities.rs` lines 19-25 for PathPattern):
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathPattern(pub String);

impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

**What to apply:** Use this pattern for `AgentRequest`, `AgentResponse`, `ReActStep` (enum, not struct), and `AgentTerminationReason` (enum). Use `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]` on all types. `AgentRequest` and `AgentResponse` get `Default` impl. `ReActStep` is an enum with variants: `Thought { content: String }`, `Action { tool: String, args: serde_json::Value }`, `Observation { result: String }`, `Error { message: String, turn: u32 }`, `Finished { answer: String }`.

---

### `crates/jadepaw-core/src/guest_exports.rs` (trait-definition, request-response)

**Analog:** `crates/jadepaw-core/src/host_functions.rs`

Additive-only trait with `#[async_trait]`, living in `jadepaw-core` with no wasmtime dependency. Same design constraints: methods may be added, never removed. Implements in `jadepaw-wasm`.

**Module doc pattern** (lines 1-9 of `host_functions.rs`):
```rust
//! Canonical guest-host communication contract.
//!
//! The `HostFunctions` trait catalogues every host function import that a
//! guest Wasm module can call. This trait is the single source of truth for
//! the guest-host interface and lives in `jadepaw-core` so that downstream
//! crates (jadepaw-agent, jadepaw-skill) can reference it without depending
//! on `jadepaw-wasm`.
```

**Trait definition pattern** (lines 24-59 of `host_functions.rs`):
```rust
use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait HostFunctions: Send + Sync {
    /// Doc comment per method.
    async fn log_message(&self, level: String, message: String) -> Result<()>;

    async fn file_read(&self, path: String) -> Result<Vec<u8>>;

    async fn file_write(&self, path: String, data: Vec<u8>) -> Result<()>;
}
```

**What to apply:** Define `GuestExports` trait with optional methods that have default LLM-fallback implementations. Methods per D-03: `evaluate_step(thought: String, observation: String) -> NextAction`, `select_tool(goal: String, available_tools: Vec<ToolDef>) -> ToolChoice`, `should_continue(turn: u32, history_summary: String) -> bool`. The `NextAction` and `ToolChoice` types can be enums defined in `agent_types.rs`. Default impls return `NextAction::from_llm()`, `ToolChoice::default()`, etc.

---

### `crates/jadepaw-agent/src/loop.rs` (service, event-driven)

**Analog:** `crates/jadepaw-wasm/src/pool.rs`

Async orchestrator with configuration struct, structured error handling via `anyhow::Result`, and a main async function that composes multiple sub-operations. Follows the `InstancePool::acquire()` pattern -- a single primary async function that takes config/state inputs and produces a result.

**Import pattern** (lines 24-34 of `pool.rs`):
```rust
use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use wasmtime::{Engine, Instance, InstancePre, Linker, Module, Store};

use crate::engine::EngineFactory;
use crate::linker::{create_linker, register_host_functions};
use crate::session::SessionState;
use jadepaw_core::SessionId;
```

**Configuration struct pattern** (lines 42-60 of `pool.rs`):
```rust
/// Configuration for creating an `InstancePool`.
#[derive(Clone)]
pub struct PoolConfig {
    pub guest_bytes: Vec<u8>,
    pub sandbox_root: PathBuf,
    pub max_concurrent: usize,
}

impl PoolConfig {
    pub fn new(guest_bytes: Vec<u8>, sandbox_root: PathBuf, max_concurrent: usize) -> Self {
        Self {
            guest_bytes,
            sandbox_root,
            max_concurrent,
        }
    }
}
```

**Async function signature pattern** (lines 189-193 of `pool.rs`):
```rust
    pub async fn acquire(
        &self,
        session_id: SessionId,
        state: SessionState,
    ) -> anyhow::Result<SessionHandle> {
```

**Error mapping pattern** (lines 200-201 of `pool.rs`):
```rust
        .map_err(|_| anyhow::anyhow!("semaphore closed -- pool is shutting down"))?;
```

**What to apply:** Define `LoopConfig` with `max_iterations: u32` (default 20), `model: String`. The main function `async fn react_loop(config: &LoopConfig, session: &mut SessionHandle, llm: &Client<Box<dyn Config>>, tx: &mpsc::Sender<ReActStep>) -> Result<Vec<ReActStep>>`. Per-turn fuel reset: `session.store_mut().set_fuel(1_000_000)`. Use `anyhow::Result` for internal operations, mapping errors to `JadepawError` variants at boundaries.

---

### `crates/jadepaw-agent/src/llm.rs` (service, streaming)

**Analog:** No direct analog in existing codebase. RESEARCH.md provides verified code examples from async-openai 0.40.2.

**Import pattern (from RESEARCH.md code example lines 282-291):**
```rust
use async_openai::{
    Client, config::Config,
    types::chat::{
        CreateChatCompletionRequest, CreateChatCompletionRequestArgs,
        ChatCompletionRequestMessage,
    },
};
use futures::StreamExt;
use tokio::sync::mpsc;
use jadepaw_core::ReActStep;
```

**Stream creation pattern (from RESEARCH.md code example lines 293-328):**
```rust
async fn stream_llm_response(
    client: &Client<Box<dyn Config>>,
    messages: Vec<ChatCompletionRequestMessage>,
    model: &str,
    tx: &mpsc::Sender<ReActStep>,
) -> Result<String> {
    let request = CreateChatCompletionRequestArgs::default()
        .model(model.to_string())
        .messages(messages)
        .build()?;

    let mut stream = client.chat().create_stream(request).await?;

    let mut full_content = String::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                for choice in response.choices {
                    if let Some(content) = choice.delta.content {
                        full_content.push_str(&content);
                    }
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    Ok(full_content)
}
```

**System prompt pattern:** Define as a hardcoded constant:
```rust
pub const REACT_SYSTEM_PROMPT: &str = "...";
```

**What to apply:** Two functions: `build_initial_messages(user_message: &str) -> Vec<ChatCompletionRequestMessage>` and `stream_llm_response(client, messages, model, tx) -> Result<String>`. No `LlmClient` trait -- use `Client<Box<dyn Config>>` directly per D-05/D-06.

---

### `crates/jadepaw-agent/src/guard.rs` (utility, event-driven)

**Analog:** `crates/jadepaw-wasm/src/limits/instance_hard.rs`

Policy boundary module -- enforces rules but does not own the core computation. Uses `tokio::select!` for the guard pattern (RESEARCH.md verified pattern).

**Module structure pattern** (from `instance_hard.rs` lines 1-13):
```rust
//! InstanceHardLimiter -- per-instance security boundary.
//!
//! Enforces a hard per-instance memory cap at 64MB (configurable).
//! Returns `Err()` (trap, Store poisoned) when the limit is exceeded.
//! ...

use wasmtime::ResourceLimiter;

#[derive(Debug, Clone)]
pub struct InstanceHardLimiter {
    max_bytes: usize,
}

impl InstanceHardLimiter {
    pub fn new(max_mb: u32) -> Self {
        // ...
    }
}
```

**tokio::select! guard pattern (from RESEARCH.md lines 362-391):**
```rust
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use jadepaw_core::{AgentTerminationReason, JadepawError, ReActStep};

pub struct GuardConfig {
    pub max_iterations: u32,
    pub wall_clock_timeout: Duration,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            wall_clock_timeout: Duration::from_secs(300),
        }
    }
}

pub async fn run_with_guard<F, Fut>(
    config: GuardConfig,
    agent_loop: F,
) -> Result<Vec<ReActStep>, JadepawError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<ReActStep>, JadepawError>>,
{
    tokio::select! {
        result = agent_loop() => {
            result
        }

        _ = sleep(config.wall_clock_timeout) => {
            Err(JadepawError::AgentTerminated {
                reason: AgentTerminationReason::WallClockTimeout {
                    elapsed: config.wall_clock_timeout,
                    max: config.wall_clock_timeout,
                },
            })
        }
    }
}
```

**What to apply:** `GuardConfig` with `Default`, `run_with_guard()` function with `tokio::select!`. Two futures race: the agent loop future (carrying its own iteration counter check), and a `tokio::time::sleep` for wall-clock timeout. Iteration limit checked inside the loop body (returns `MaxIterationsReached` error from within the loop).

---

### `crates/jadepaw-agent/src/stream.rs` (utility, streaming)

**Analog:** No direct analog in existing codebase. RESEARCH.md provides verified axum SSE + tokio_stream patterns.

**Import and setup pattern (from RESEARCH.md lines 527-558):**
```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

async fn sse_handler() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::channel::<String>(256);

    tokio::spawn(async move {
        for i in 0..10 {
            let msg = format!("message {}", i);
            if tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    let stream = ReceiverStream::new(rx).map(|msg| {
        Ok(Event::default()
            .event("thought")
            .data(msg))
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
    )
}
```

**SSE event naming pattern (D-14):**
- `event: thought` -- Thought ReActStep
- `event: action` -- Action ReActStep
- `event: observation` -- Observation ReActStep
- `event: token` -- Individual LLM token (real-time streaming)
- `event: done` -- Final answer + execution trace

**What to apply:** `create_sse_channel() -> (mpsc::Sender<ReActStep>, impl Stream<Item = Result<Event, Infallible>>)` that creates channel, maps ReActStep to SSE Event enum variants, returns Sender and Stream. Use channel capacity 256 for backpressure. Apply `Event::default().event(name).data(json_string)` (D-14: use ReActStep serde_json serialization for data).

---

### `crates/jadepaw-agent/src/lib.rs` (modify) (facade, request-response)

**Analog:** `crates/jadepaw-agent/src/lib.rs` (self) + `crates/jadepaw-wasm/src/lib.rs`

**Module declaration pattern** (from `jadepaw-wasm/src/lib.rs` lines 23-39):
```rust
pub mod capability;
pub mod engine;
pub mod epoch;
pub mod host;
pub mod limits;
pub mod linker;
pub mod path;
pub mod pool;
pub mod session;

pub use engine::EngineFactory;
pub use epoch::{start_epoch_ticker, EpochTickerGuard};
pub use limits::{InstanceHardLimiter, TenantQuotaLimiter};
pub use linker::{create_linker, register_host_functions};
pub use path::{normalize_path, validate_sandbox_path};
pub use pool::{InstancePool, PoolConfig, SessionHandle};
pub use session::{SessionLimits, SessionState};
```

**What to apply:** Add module declarations for `loop`, `llm`, `guard`, `stream`. The `run_agent()` function (D-13) is declared here as the primary entry point:
```rust
pub async fn run_agent(
    req: AgentRequest,
    pool: Arc<InstancePool>,
    llm: Client<Box<dyn Config>>,
) -> Result<AgentResponse, JadepawError>;
```
Re-export key public types: `run_agent`, `GuardConfig`, `LoopConfig`, `create_sse_channel`.

---

### `crates/jadepaw-core/src/error.rs` (modify) (model, request-response)

**Analog:** `crates/jadepaw-core/src/error.rs` (self -- extend existing enum)

**Existing enum pattern** (lines 12-33 of `error.rs`):
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JadepawError {
    CapabilityDenied {
        operation: String,
        detail: String,
    },

    TrapError {
        message: String,
    },

    PathValidationError {
        path: String,
        reason: String,
    },
}
```

**Constructor pattern** (lines 37-57 of `error.rs`):
```rust
impl JadepawError {
    pub fn capability_denied(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::CapabilityDenied {
            operation: operation.into(),
            detail: detail.into(),
        }
    }
}
```

**Display impl pattern** (lines 60-79 of `error.rs`):
```rust
impl fmt::Display for JadepawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDenied { operation, detail } => {
                write!(f, "capability denied: ...")
            }
            // ...
        }
    }
}
```

**What to apply:** Add a `AgentTerminated` variant that wraps `AgentTerminationReason`:
```rust
    AgentTerminated {
        reason: AgentTerminationReason,
    },
```
Define `AgentTerminationReason` enum in `agent_types.rs` (not in error.rs) with variants `MaxIterationsReached { iter: u32, max: u32 }`, `WallClockTimeout { elapsed: Duration, max: Duration }`, `WasmTrap { reason: String, turn: u32 }`. Then add constructor method and Display case. Note: `Duration` from `std::time` does not impl `PartialEq` / `Eq` / `Clone` (already requires std). Use `Duration` directly since the error type already uses `std::fmt`.

---

### `crates/jadepaw-core/src/lib.rs` (modify) (facade, request-response)

**Analog:** `crates/jadepaw-core/src/lib.rs` (self -- extend existing mods)

**Existing module/export pattern** (lines 20-29 of `lib.rs`):
```rust
pub mod capabilities;
pub mod error;
pub mod host_functions;
pub mod types;

pub use capabilities::{DomainPattern, InstanceCapabilities, PathPattern};
pub use error::{JadepawError, Result};
pub use host_functions::HostFunctions;
pub use types::{SessionId, TenantId, ToolId};
```

**What to apply:** Add `pub mod agent_types;` and `pub mod guest_exports;`. Add re-exports:
```rust
pub use agent_types::{AgentRequest, AgentResponse, AgentTerminationReason, ReActStep};
pub use guest_exports::GuestExports;
```

---

### `Cargo.toml` (workspace, modify) (config, config)

**Analog:** `Cargo.toml` (self -- line 44, extend existing dep)

**Existing pattern** (line 44 of workspace `Cargo.toml`):
```toml
async-openai = "0.40"
```

**What to apply:** Change to:
```toml
async-openai = { version = "0.40", features = ["chat-completion"] }
```

Also add to `jadepaw-agent/Cargo.toml` dependencies:
```toml
tokio-stream = "0.1"
futures = "0.3"
```

---

## Shared Patterns

### Error Handling
**Source:** `crates/jadepaw-core/src/error.rs`
**Apply to:** All jadepaw-agent modules
```rust
// Internal operations use anyhow::Result for flexibility,
// mapping to JadepawError at public API boundaries:
fn internal_op() -> anyhow::Result<Data> {
    // ...
}

pub async fn public_api() -> jadepaw_core::Result<Output> {
    internal_op().await.map_err(|e| JadepawError::AgentTerminated {
        reason: AgentTerminationReason::WasmTrap {
            reason: e.to_string(),
            turn: current_turn,
        },
    })
}
```

### Async Test Pattern
**Source:** `crates/jadepaw-wasm/tests/pool.rs` and `tests/limits.rs`
**Apply to:** All test files in `crates/jadepaw-agent/tests/`
```rust
#[tokio::test(flavor = "multi_thread")]
async fn test_react_loop_multiple_cycles() {
    // ...
}
```
Use `#[tokio::test(flavor = "multi_thread")]` per CLAUDE.md convention for all wasmtime/async tests.

### Module Doc Comments
**Source:** `crates/jadepaw-core/src/lib.rs` (lines 1-19), individual modules
**Apply to:** All new module files
```rust
//! Module purpose -- one-line summary.
//!
//! Detailed description of what this module does, its design constraints,
//! and how it fits into the larger system.
//!
//! # Design (D-XX)
//!
//! - Bullet points of key design decisions
```

### Configuration Struct Pattern
**Source:** `crates/jadepaw-wasm/src/pool.rs` (lines 42-60)
**Apply to:** `GuardConfig`, `LoopConfig`
```rust
#[derive(Clone)]
pub struct XxxConfig {
    pub field: Type,
}

impl Default for XxxConfig {
    fn default() -> Self {
        Self { field: default_value }
    }
}
```

### Public Type Re-export
**Source:** `crates/jadepaw-wasm/src/lib.rs` (lines 33-39)
**Apply to:** `crates/jadepaw-agent/src/lib.rs`
```rust
pub use guard::GuardConfig;
pub use loop::LoopConfig;
pub use stream::create_sse_channel;
```

## No Analog Found

Files with no close match in the codebase (planner should use RESEARCH.md patterns instead):

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| `crates/jadepaw-agent/src/llm.rs` | service | streaming | No existing LLM integration in the codebase. async-openai 0.40.2 patterns are fully specified in RESEARCH.md (lines 279-328 for streaming, lines 474-520 for verified API). |
| `crates/jadepaw-agent/src/stream.rs` | utility | streaming | No existing SSE/mpsc relay in the codebase. axum Sse + tokio_stream patterns are fully specified in RESEARCH.md (lines 527-558). |

Both files have comprehensive, verified code examples in RESEARCH.md that serve as effective templates.

## Metadata

**Analog search scope:** `crates/jadepaw-core/src/`, `crates/jadepaw-wasm/src/`, `crates/jadepaw-wasm/tests/`, `crates/jadepaw-agent/`
**Files scanned:** 11
**Pattern extraction date:** 2026-06-01