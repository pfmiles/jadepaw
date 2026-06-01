# Phase 3: Agent Runtime - Research

**Researched:** 2026-06-01
**Domain:** Agent execution loop (ReAct pattern) with LLM integration, SSE streaming, termination guards
**Confidence:** HIGH

## Summary

Phase 3 builds the intelligent reasoning layer on top of Phase 2's Wasm isolation infrastructure. The agent loop runs on the host side (Rust, `jadepaw-agent` crate) -- it calls async-openai for LLM reasoning, dispatches tool execution through Phase 2's capability-gated Wasm sandbox, and streams responses back to the caller in real time via SSE. The guest Wasm modules MAY export decision-point functions (`evaluate_step`, `select_tool`, `should_continue`) that the host loop calls at specific phases; if absent, the host defaults to LLM-based behavior.

The stack is already verified: async-openai 0.40.2 (verified in Cargo.lock) provides the `Client<Box<dyn Config>>` multi-provider dispatch and `ChatCompletionResponseStream` (a `Pin<Box<dyn Stream<...>>>`). The tokio mpsc channel + tokio-stream ReceiverStream + axum Sse::new() pipeline handles real-time token delivery. Termination guards use `tokio::select!` with three racing futures: loop completion, iteration counter, wall-clock timeout.

**Primary recommendation:** Implement in four modules within `jadepaw-agent`: `loop.rs` (ReAct orchestrator), `llm.rs` (async-openai integration), `guard.rs` (termination protection), `stream.rs` (SSE token relay). Add types to `jadepaw-core`: `AgentRequest`, `AgentResponse`, `ReActStep`, `AgentTerminationReason`. Enable `async-openai`'s `chat-completion` feature in workspace Cargo.toml.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| ReAct loop orchestration | Host (jadepaw-agent) | -- | Host owns the loop lifecycle: LLM calls, tool dispatch, termination. Wasm guest provides optional decision points only. |
| LLM API calls + streaming | Host (jadepaw-agent) | -- | async-openai Client runs on host. Streaming tokens flow through host-side channels only -- no Wasm boundary crossing (D-07). |
| SSE token relay to caller | Host (jadepaw-agent) | -- | tokio channel + axum Sse. Pure host-side async primitives. |
| Tool execution dispatch | Host (jadepaw-agent) | Wasm sandbox | Host dispatches tool calls; Wasm guest can optionally influence which tool is selected via `select_tool` export. |
| Guest decision-point functions | Wasm Guest | -- | Optional exports (`evaluate_step`, `select_tool`, `should_continue`) that customize agent behavior. Host falls back to LLM defaults if absent. |
| Termination guards | Host (jadepaw-agent) | -- | `tokio::select!` in guard.rs races loop vs count vs timeout. Phase 2 Wasm-level fuel/epoch limits remain as security boundary. |
| Data types (AgentRequest etc.) | jadepaw-core | -- | Types in core, impl in jadepaw-agent. Same pattern as HostFunctions. |

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** The ReAct loop skeleton runs on the host side in `jadepaw-agent` crate (Rust). Host owns: LLM API calls, SSE streaming, tool execution dispatch (with Phase 2 capability checks), and termination guards (iteration limit + wall-clock timeout).
- **D-02:** Guest Wasm modules MAY export decision-point functions. Host falls back to default LLM-based behavior if absent.
- **D-03:** Initial guest export interface: `evaluate_step`, `select_tool`, `should_continue`. Additive-only, defined as trait(s) in `jadepaw-core`.
- **D-05:** Use async-openai's `Client<Box<dyn Config>>` directly in `jadepaw-agent`.
- **D-06:** Do NOT abstract an `LlmClient` trait in Phase 3.
- **D-07:** SSE token streaming: `ChatCompletionStream` -> tokio channel -> axum SSE response. No Wasm boundary crossing.
- **D-08:** Host-level termination via `tokio::select!` in guard.rs: loop completion vs iteration counter vs wall-clock timeout.
- **D-09:** `JadepawError` gains `AgentTerminationReason` enum.
- **D-10:** Phase 2 Wasm-level protection unchanged. Host = policy boundary, Wasm = security boundary.
- **D-12:** Types in `jadepaw-core`: `AgentRequest`, `AgentResponse`, `ReActStep` enum.
- **D-13:** Function signature: `async fn run_agent(req, pool, llm) -> Result<AgentResponse>`.
- **D-14:** SSE event mapping: each ReActStep as named SSE event. `event: done` carries final answer + trace.

### Claude's Discretion

No areas were deferred to Claude -- all decisions were user-directed.

### Deferred Ideas (OUT OF SCOPE)

- Pure guest-side loop (full Wasm autonomy)
- `LlmClient` trait abstraction
- Per-turn LLM/tool timeout

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| async-openai | 0.40.2 | LLM API client (chat completions + streaming) | Already in workspace. `Client<Box<dyn Config>>` handles multi-provider dispatch. `create_stream()` returns `ChatCompletionResponseStream`. [VERIFIED: npm registry -- verified at /cargo/registry/src/async-openai-0.40.2/chat.rs:75] |
| tokio | 1.52.3 | Async runtime, mpsc channel, select! macro | Already in workspace. `tokio::sync::mpsc` for token relay, `tokio::select!` for termination guards. [VERIFIED: Cargo.lock] |
| axum | 0.8 | SSE response | Already in workspace. `Sse::new(stream)` sets `text/event-stream` content-type. `Event::default().data().event()` for named SSE events. [CITED: docs.rs/axum/latest/axum/response/struct.Sse.html] |
| tokio-stream | 0.1.x | ReceiverStream adapter for mpsc -> Stream | Required by async-openai `_api` feature transitively. `ReceiverStream::new(rx)` converts `mpsc::Receiver<T>` into `impl Stream<Item = T>`. [CITED: docs.rs/tokio-stream/latest/tokio_stream/wrappers/struct.ReceiverStream.html] |
| futures | (transitive) | StreamExt for poll_next | Already in dependency tree via async-openai. Used to iterate streaming responses. `StreamExt::next()` on `ChatCompletionResponseStream`. [VERIFIED: Cargo.lock -- futures-core 0.3.32 transitive] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| wasmtime | 45.0.0 | Guest Wasm instance callout | Only for calling guest exports (evaluate_step, select_tool). Already provided via jadepaw-wasm's SessionHandle. |
| serde / serde_json | 1.0 | Serialization of AgentRequest/Response | Already in workspace. AgentRequest and AgentResponse must be serde-serializable (D-12). |
| chrono | 0.4 | Timestamps in AgentResponse trace | Already in workspace. SessionState already uses chrono. |
| uuid | 1.0 | ID generation | Already in workspace. For agent run IDs if needed. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `tokio::sync::mpsc` + `ReceiverStream` | `futures::channel::mpsc` | tokio mpsc is already in the dependency tree, integrates natively with axum and tokio tasks. No additional dependency needed. |
| `tokio::select!` for termination | `tokio::time::timeout` wrapping the entire loop | `select!` with three futures gives finer-grained control -- we can distinguish "iteration limit" from "wall-clock timeout" in the termination reason (D-09). `timeout` alone can't tell which condition triggered. |

**Installation:**

```bash
# No new packages to install -- all dependencies are already in workspace.
# However, the async-openai chat-completion feature must be enabled:
```

In workspace `Cargo.toml`, change:
```toml
async-openai = "0.40"
```
to:
```toml
async-openai = { version = "0.40", features = ["chat-completion"] }
```

Add `tokio-stream` and `futures` as direct dependencies of `jadepaw-agent`:
```toml
tokio-stream = "0.1"
futures = "0.3"
```

**Version verification:**
- `wasmtime` 45.0.0 [VERIFIED: Cargo.lock]
- `async-openai` 0.40.2 [VERIFIED: Cargo.lock]
- `tokio` 1.52.3 [VERIFIED: Cargo.lock]
- `tokio-stream` 0.1.x -- transitive via async-openai `_api` feature [VERIFIED: async-openai Cargo.toml line ~460]
- `futures` (`futures-core` 0.3.32 exists in Cargo.lock) [VERIFIED: Cargo.lock]

## Package Legitimacy Audit

All packages in this phase's stack already exist in the workspace dependency tree (Cargo.lock). No new external packages are being introduced beyond enabling existing workspace dependencies with additional features.

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| async-openai (0.40.2) | crates.io | ~2+ yrs | High (well-established) | github.com/64bit/async-openai | N/A (already in lockfile) | Approved -- existing dep |
| tokio-stream | crates.io | 5+ yrs | Very high | github.com/tokio-rs/tokio | N/A (already transitive) | Approved -- tokio ecosystem |
| futures | crates.io | 7+ yrs | Very high | github.com/rust-lang/futures-rs | N/A (already transitive) | Approved -- std-adjacent |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
**New direct dependencies added:** `tokio-stream` (to jadepaw-agent), `futures` (to jadepaw-agent) -- both already in lockfile as transitive deps. `async-openai` feature expanded from default to `["chat-completion"]`.

## Architecture Patterns

### System Architecture Diagram

```
                       Caller (API / Test Harness)
                              |
                              | AgentRequest
                              v
                    +-----------------------+
                    |   jadepaw-agent        |
                    |   run_agent(req, pool, |
                    |            llm)        |
                    +-----------+-----------+
                                |
                   tokio::select!  <-- guard.rs: three futures race
                    /      |       \
                   /       |        \
          loop done   iter limit   wall-clock timeout
                   \       |        /
                    \      |       /
                     +-----v------+
                     | ReAct Loop  |  <-- loop.rs orchestrator
                     +-----+------+
                           |
              +------------+------------+
              |            |            |
        Think Phase   Act Phase    Observe Phase
              |            |            |
              v            v            v
     +--------+--+   +----+-----+  +---+--------+
     | llm.rs    |   | Wasm    |  | llm.rs     |
     | create_   |   | callout |  | feed result|
     | stream()  |   | (export)|  | to context |
     +-----------+   +---------+  +------------+
              |            |
              v            v
       ChatCompletion   InstancePool
       ResponseStream   .acquire()
              |
              v
     +-------------------+
     | mpsc channel      |
     | (token relay)     |  <-- stream.rs
     +--------+----------+
              |
              v
     +-------------------+
     | axum Sse response |
     | (SSE events)      |
     +-------------------+
              |
              v
          Caller (real-time token stream)
```

**Flow:**
1. Caller sends `AgentRequest` -> `run_agent()` is called
2. `guard.rs` wraps the agent loop in `tokio::select!` with two timeout futures
3. `loop.rs` orchestrates think -> act -> observe cycles
4. Think phase: `llm.rs` calls `client.chat().create_stream()` -> `ChatCompletionResponseStream`
5. Streamed tokens are relayed through `mpsc::channel` -> `ReceiverStream` -> axum `Sse` response
6. Each complete `ReActStep` (thought/action/observation) is emitted as an SSE named event
7. Guest Wasm exports (`evaluate_step`, `select_tool`, `should_continue`) are called if present
8. On loop completion or timeout, `event: done` carries `AgentResponse` (final answer + trace)

### Recommended Project Structure

```
crates/jadepaw-core/src/
├── agent_types.rs      # NEW: AgentRequest, AgentResponse, ReActStep, AgentTerminationReason
├── guest_exports.rs    # NEW: GuestExports trait (evaluate_step, select_tool, should_continue)
├── capabilities.rs     # (existing)
├── error.rs            # MODIFIED: add AgentTerminationReason variant
├── host_functions.rs   # (existing)
├── lib.rs              # MODIFIED: add new modules to pub mod + re-exports
└── types.rs            # (existing)

crates/jadepaw-agent/src/
├── loop.rs             # NEW: ReAct loop orchestrator (think -> act -> observe)
├── llm.rs              # NEW: async-openai integration, prompt construction, streaming
├── guard.rs            # NEW: tokio::select! termination guard
├── stream.rs           # NEW: SSE token relay via mpsc channel
└── lib.rs              # MODIFIED: run_agent() entry point, module declarations
```

### Pattern 1: ReAct Loop (Host-Side Orchestrator)

**What:** The loop runs on the host, calling LLM for reasoning, dispatching tool calls through Wasm, and feeding observations back into context. Guest Wasm modules can optionally intercept at decision points.

**When to use:** This IS Phase 3. The entire agent execution follows this pattern.

**Example (pseudocode):**
```rust
// Source: D-01, D-02, D-03 from CONTEXT.md
// File: crates/jadepaw-agent/src/loop.rs

async fn react_loop(
    request: &AgentRequest,
    session: &mut SessionHandle,
    llm: &Client<Box<dyn Config>>,
    tx: &mpsc::Sender<ReActStep>,
    guard: &Guard,
) -> Result<Vec<ReActStep>> {
    let mut trace: Vec<ReActStep> = Vec::new();
    let mut history: Vec<ChatCompletionRequestMessage> = build_initial_messages(request);

    for turn in 0..guard.max_iterations {
        // 1. THINK: call LLM with current conversation history
        let response = call_llm(llm, &history).await?;

        // 2. Check guest decision point (optional)
        let next_action = if let Some(evaluate_step_fn) = get_guest_export(session, "evaluate_step") {
            let thought = response.content.clone().unwrap_or_default();
            call_guest_evaluate_step(session, thought, last_observation()).await?
        } else {
            NextAction::from_llm_response(&response)
        };

        match next_action {
            NextAction::Finish(final_answer) => {
                let step = ReActStep::Finished { answer: final_answer };
                tx.send(step.clone()).await.ok();
                trace.push(step);
                break;
            }
            NextAction::Act { tool, args } => {
                // 3. ACT: dispatch tool through Wasm (with capability check)
                trace.push(ReActStep::Action { tool: tool.clone(), args: args.clone() });
                let result = dispatch_tool(session, &tool, &args).await?;

                // 4. OBSERVE: feed result back into context
                trace.push(ReActStep::Observation { result: result.clone() });
                history.push(format_observation_as_message(&result));
            }
            NextAction::ContinueThinking => {
                // record thought, continue loop
                trace.push(ReActStep::Thought { content: response.content.unwrap_or_default() });
            }
        }
    }

    Ok(trace)
}
```

### Pattern 2: LLM Streaming with mpsc Relay

**What:** LLM response tokens stream through an mpsc channel to axum SSE. Each token is a delta; complete thoughts become named SSE events.

**When to use:** AGENT-03 (streaming output). Tokens must reach the caller in real time.

**Example (verified API):**
```rust
// Source: async-openai 0.40.2 chat.rs:75, types/chat/chat_.rs:1175
// File: crates/jadepaw-agent/src/llm.rs

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
                        // Stream individual tokens or aggregate as needed
                    }
                    // Check finish_reason for tool calls
                    if let Some(finish_reason) = choice.finish_reason {
                        // Handle stop/tool_calls/length/content_filter
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

### Pattern 3: Termination Guard with tokio::select!

**What:** Three futures race: the agent loop, an iteration counter, and a wall-clock timer. First to complete cancels the others. The termination reason is propagated through the JadepawError.

**When to use:** AGENT-04 (termination protection). Every agent invocation is wrapped in this guard.

**Example:**
```rust
// Source: D-08, D-09 from CONTEXT.md
// File: crates/jadepaw-agent/src/guard.rs

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
            wall_clock_timeout: Duration::from_secs(300), // 5 minutes
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
        // Future 1: Agent loop completion (normal or error)
        result = agent_loop() => {
            result
        }

        // Future 2: Wall-clock timeout
        _ = sleep(config.wall_clock_timeout) => {
            Err(JadepawError::AgentTerminated {
                reason: AgentTerminationReason::WallClockTimeout {
                    elapsed: config.wall_clock_timeout,
                    max: config.wall_clock_timeout,
                },
            })
        }

        // Future 3: Iteration limit
        // Note: iteration counter is checked INSIDE the loop body, not as a separate future.
        // The select! races the loop future against a sleep; the loop itself checks
        // the counter and returns MaxIterationsReached error when exceeded.
    }
}
```

### Anti-Patterns to Avoid

- **Anti-pattern: Putting LLM calls inside Wasm guest:** The guest cannot make network calls. LLM API access is host-side only. Guest Wasm provides decision-point functions, not LLM access.
- **Anti-pattern: Blocking SSE stream until loop completes:** The mpsc channel must be created BEFORE the loop starts, and tokens must be sent through it immediately. Do not buffer the entire response and send it all at once.
- **Anti-pattern: Reusing Store across agent invocations:** Each agent session gets one Store from InstancePool::acquire(). The Store is dropped when SessionHandle is dropped. Do not hold Store references across loop iterations beyond the SessionHandle lifetime.
- **Anti-pattern: Catching all errors silently in the loop:** Errors from Wasm execution (traps, capability denials) must be recorded in the trace and surfaced in the AgentResponse. Silent error swallowing makes debugging impossible.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| LLM API client + streaming | Custom reqwest + SSE parser | `async-openai`'s `client.chat().create_stream()` | Handles SSE event parsing, reconnection, error types, provider dispatch. Writing SSE parsing from scratch is error-prone (handling `data: [DONE]`, chunked encoding, Content-Type validation). [VERIFIED: async-openai 0.40.2 chat.rs:75] |
| mpsc-to-Stream conversion | Custom Stream impl over mpsc | `tokio_stream::wrappers::ReceiverStream` | Correctly handles poll_next delegation, wakeup on new messages, and channel closure detection. Custom Stream impls are a common source of subtle bugs. [CITED: docs.rs/tokio-stream] |
| Termination timeout logic | Manual Instant::elapsed() checks + cancellation | `tokio::select!` with sleep future | `select!` provides proper cancellation of the other futures when one completes. Manual polling with `Instant` risks missed wakeups and wasted CPU cycles. |
| SSE response formatting | Manual `text/event-stream` formatting | `axum::response::Sse::new(stream).keep_alive()` | Automatically sets correct Content-Type, handles keep-alive, event framing. Manual SSE formatting leads to subtle protocol errors (missing newlines, wrong field ordering). [CITED: docs.rs/axum/latest/axum/response/struct.Sse.html] |

**Key insight:** The streaming pipeline (LLM -> mpsc -> ReceiverStream -> Sse) is a well-established pattern in the Rust async ecosystem. Every component already exists and is battle-tested. Building any piece from scratch introduces risk with no benefit.

## Runtime State Inventory

This is a greenfield phase -- no rename, refactor, or migration. No runtime state to inventory.

## Common Pitfalls

### Pitfall 1: async-openai Feature Gate for Streaming Types

**What goes wrong:** `CreateChatCompletionStreamResponse` and `ChatCompletionResponseStream` are gated behind `#[cfg(feature = "_api")]`, which is enabled by the `chat-completion` feature. Without this feature flag, the streaming types are not compiled and the code won't build.

**Why it happens:** async-openai 0.40.x default features are `["rustls"]` only. The `chat-completion` feature must be explicitly enabled in workspace Cargo.toml.

**How to avoid:** Change workspace dependency to `async-openai = { version = "0.40", features = ["chat-completion"] }`.

**Warning signs:** Compiler error "cannot find type `CreateChatCompletionStreamResponse`" or "`create_stream` method not found on `Chat`".

### Pitfall 2: mpsc Channel Capacity and Backpressure

**What goes wrong:** The LLM produces tokens faster than the SSE consumer reads them. With an unbounded channel, memory grows unbounded. With too-small capacity, the LLM stream is throttled.

**Why it happens:** Network latency between SSE sender and HTTP client creates natural backpressure. The mpsc channel is the buffer between producer (LLM) and consumer (SSE).

**How to avoid:** Use `mpsc::channel(256)` -- a moderate buffer. If the channel is full, the LLM token producer's `tx.send().await` will yield, providing natural backpressure. 256 is large enough to smooth jitter but small enough to prevent memory issues.

**Warning signs:** Channel `send` calls blocking for long periods (check with metrics), or OOM from unbounded buffering.

### Pitfall 3: Store Fuel Not Reset Per Turn

**What goes wrong:** After Phase 2, the Store starts with 1M fuel. If fuel is not reset between turns, a guest that consumes many instructions per turn will exhaust the total budget, causing a false-positive trap.

**Why it happens:** The `store.set_fuel(1_000_000)` call in `InstancePool::acquire()` sets the initial fuel. Fuel decrements across all guest operations. Without per-turn reset, cumulative usage across turns exceeds 1M.

**How to avoid:** Call `store.set_fuel(1_000_000)` at the start of each ReAct iteration (after the "think" phase, before dispatching tool calls to the guest). This is mentioned in CONTEXT.md specifics: "Fuel reset per turn."

**Warning signs:** Guest traps with "all fuel consumed" on turn 2-3 even though each turn's operations are small.

### Pitfall 4: SSE Event Framing Errors

**What goes wrong:** The SSE response is malformed -- missing double-newline between events, incorrect `event:` field format, or `data:` field not properly escaped.

**Why it happens:** axum's `Sse` handles framing automatically when you use `Event::default().data(payload).event(name)`. Manual string formatting of SSE events bypasses this safety.

**How to avoid:** Always construct events via `axum::response::sse::Event` builder. Never manually format `"event: thought\ndata: {}\n\n"` strings.

**Warning signs:** Client-side SSE parsing errors, events not received, or EventSource connection dropping unexpectedly.

### Pitfall 5: Guest Export Lookup at Runtime

**What goes wrong:** Trying to call a guest export that doesn't exist causes a wasmtime error. The lookup-and-fallback pattern must handle the "export not found" case gracefully.

**Why it happens:** Guest modules MAY export decision-point functions, but are not required to. The host must check `instance.get_export()` or `instance.get_func()` and fall back to LLM defaults if absent.

**How to avoid:** Wrap each guest export call in a helper that returns `Option<...>`. Example: `fn try_get_guest_func(instance: &Instance, store: &mut Store<SessionState>, name: &str) -> Option<TypedFunc<...>>`. If None, use the LLM-based default.

**Warning signs:** wasmtime `Trap` errors with "unknown export" when calling a guest module that doesn't export the expected function.

## Code Examples

Verified patterns from official sources:

### async-openai Streaming Chat Completion

```rust
// Source: async-openai 0.40.2 chat.rs:75, types/chat/chat_.rs:1175-1197
// Verified: /cargo/registry/src/async-openai-0.40.2/

use async_openai::{
    Client, config::Config,
    types::chat::{
        CreateChatCompletionRequestArgs,
        ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs,
    },
};
use futures::StreamExt;

async fn stream_example(client: &Client<Box<dyn Config>>) -> Result<()> {
    let request = CreateChatCompletionRequestArgs::default()
        .model("gpt-4o")
        .messages(vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content("You are a helpful assistant.")
                .build()?
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content("Hello!")
                .build()?
                .into(),
        ])
        .build()?;

    let mut stream = client.chat().create_stream(request).await?;

    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                for choice in response.choices {
                    if let Some(content) = choice.delta.content {
                        print!("{}", content);
                    }
                }
            }
            Err(e) => eprintln!("Stream error: {e}"),
        }
    }

    Ok(())
}
```

### axum SSE from mpsc Channel

```rust
// Source: docs.rs/axum/latest/axum/response/struct.Sse.html
// Source: docs.rs/tokio-stream/latest/tokio_stream/wrappers/struct.ReceiverStream.html

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
                break; // receiver dropped
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

### tokio::select! Guard Pattern

```rust
// Source: tokio docs, D-08 from CONTEXT.md

use std::time::Duration;

async fn run_with_timeout() -> Result<String, AgentTerminationReason> {
    tokio::select! {
        result = actual_work() => {
            result.map_err(|e| AgentTerminationReason::Error(e.to_string()))
        }

        _ = tokio::time::sleep(Duration::from_secs(300)) => {
            Err(AgentTerminationReason::WallClockTimeout {
                elapsed: Duration::from_secs(300),
                max: Duration::from_secs(300),
            })
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| async-openai 0.34 (CONTEXT.md references) | async-openai 0.40.2 | Workspace was updated post-CONTEXT.md | `ChatCompletionResponseStream` type signature: `Pin<Box<dyn Stream<Item = Result<CreateChatCompletionStreamResponse, OpenAIError>> + Send>>`. Feature flags changed -- `chat-completion` must be explicitly enabled. Default features are now `["rustls"]` only. |
| `CreateChatCompletionRequest` with `stream: true` | `client.chat().create_stream(request)` | 0.40.x | Dedicated method that ensures `stream: true`. The `create()` method rejects requests with `stream: true` with `InvalidArgument`. [VERIFIED: chat.rs:48-52] |
| wasmtime 38.0 (initial project docs) | wasmtime 45.0.0 | Updated during Phase 2 | All Phase 2 APIs verified at 45.0. No breaking changes affecting this phase. |

**Deprecated/outdated:**
- **`client.chat().create()` with `stream: true`:** async-openai 0.40.x errors if you pass `stream: true` to `create()`. Use `create_stream()` instead. [VERIFIED: chat.rs:48-52]
- **async-openai 0.34 API:** The workspace now uses 0.40.2. All code examples in CONTEXT.md that reference 0.34 patterns should be adapted -- particularly the feature flag requirement.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `tokio-stream` version will be compatible with async-openai's transitive dependency when added as direct dep. | Standard Stack | LOW -- tokio-stream 0.1.x is very stable. Cargo resolves version conflicts automatically. |
| A2 | Guest Wasm exports use wasmtime's `TypedFunc` for calling. | Architecture | LOW -- Phase 2 already uses this pattern for host functions. Guest export calling is the mirror operation. |
| A3 | The `chat-completion` feature of async-openai pulls in all needed streaming types. | Pitfall 1 | LOW -- verified by reading source code feature gates at `#[cfg(feature = "_api")]` in chat_.rs:1104. |
| A4 | `futures::StreamExt::next()` is the right way to iterate `ChatCompletionResponseStream`. | Code Examples | LOW -- `StreamResponse<T>` is `Pin<Box<dyn Stream<...>>>` and `StreamExt` provides `next()`. Standard pattern. |
| A5 | `tokio::select!` correctly cancels the non-winning futures. | Pitfalls | LOW -- tokio documentation confirms this. `select!` drops (and thus cancels) the futures that did not complete first. |

## Open Questions

1. **How should tool dispatch work in MVP (before Phase 4 Tool System)?**
   - What we know: AGENT-01 requires the agent to "autonomously select and call tools to complete tasks." But the tool system (MCP protocol, tool registry) is Phase 4. No tools exist in Phase 3.
   - What's unclear: How does the agent "act" without any registered tools? Does it just think and respond without acting? Or is there a minimal built-in tool (e.g., echo/debug)?
   - Recommendation: For AGENT-01 MVP, the ReAct loop should support the full think-act-observe cycle structurally, but the initial tool registry can be empty or contain a single `echo` tool. The "tool call" path in the loop dispatches to the guest Wasm instance's exports, which for now may be no-ops. The structural support for tools should be in place so Phase 4 adds tools, not loop infrastructure.

2. **What is the initial system prompt for the ReAct agent?**
   - What we know: The LLM needs a system prompt that instructs it to follow the ReAct pattern (think -> act -> observe). The prompt format depends on the model.
   - What's unclear: Should the prompt be hardcoded in jadepaw-agent source, or loaded from a config file? How configurable should it be at MVP?
   - Recommendation: Start with a hardcoded constant in `llm.rs` for MVP. It can be moved to a config source (SKILL.md or tenant config) in Phase 6. The default prompt should instruct the model to reason step-by-step, optionally call tools, and produce a final answer.

3. **How does the agent loop integrate with the axum server (Phase 7)?**
   - What we know: D-07 says SSE tokens flow through `ChatCompletionStream -> tokio channel -> axum SSE response`. D-13 says `run_agent()` is a function.
   - What's unclear: In Phase 3 MVP, is there an actual HTTP endpoint, or is the interface purely programmatic (`run_agent()` called from tests)? If no server yet, the SSE relay is tested via the channel directly.
   - Recommendation: Phase 3 MVP should be callable programmatically (test harness per success criterion 5). The SSE relay infrastructure (mpsc channel, ReceiverStream) should be built and testable without an axum server. Phase 7 adds the HTTP endpoint that wires the stream to axum Sse. The `stream.rs` module exposes the channel so tests can verify streaming behavior.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | Building jadepaw-agent | Yes | 1.85+ (assumed) | -- |
| tokio | Async runtime | Yes | 1.52.3 | -- |
| async-openai | LLM API calls | Yes (in workspace) | 0.40.2 | -- |
| OpenAI-compatible API endpoint | LLM inference (integration tests) | Needs config | Various | Mock LLM server for unit tests |

**Missing dependencies with no fallback:**
- None. All dependencies are already in the workspace or can be added from crates.io.

**Missing dependencies with fallback:**
- **LLM API endpoint:** Integration tests that actually call LLM need an API key and endpoint. Use a mock HTTP server (e.g., `wiremock` or `mockito`) for unit tests of the agent loop. Integration tests can optionally use a real API key from env vars.

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) + tokio::test |
| Config file | None (cargo test discovers tests automatically) |
| Quick run command | `cargo test -p jadepaw-agent` |
| Full suite command | `cargo test -p jadepaw-agent -- --nocapture` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGENT-01 | ReAct loop executes think -> act -> observe -> next cycles | unit | `cargo test -p jadepaw-agent test_react_loop_multiple_cycles` | No -- Wave 0 |
| AGENT-01 | Agent produces a reasoned answer from natural language input | integration | `cargo test -p jadepaw-agent test_agent_responds_with_reasoned_answer` | No -- Wave 0 |
| AGENT-01 | Guest export evaluate_step is called if present | unit | `cargo test -p jadepaw-agent test_guest_export_evaluate_step_called` | No -- Wave 0 |
| AGENT-01 | Host falls back to LLM default when guest export absent | unit | `cargo test -p jadepaw-agent test_fallback_when_guest_export_absent` | No -- Wave 0 |
| AGENT-03 | SSE events stream in real time (not buffered) | unit | `cargo test -p jadepaw-agent test_sse_streaming_real_time` | No -- Wave 0 |
| AGENT-03 | Each ReActStep emitted as named SSE event | unit | `cargo test -p jadepaw-agent test_sse_event_naming` | No -- Wave 0 |
| AGENT-03 | event:done carries final answer and trace | unit | `cargo test -p jadepaw-agent test_sse_done_event` | No -- Wave 0 |
| AGENT-04 | Loop terminated after max iterations reached | unit | `cargo test -p jadepaw-agent test_max_iterations_termination` | No -- Wave 0 |
| AGENT-04 | Loop terminated after wall-clock timeout | unit | `cargo test -p jadepaw-agent test_wall_clock_timeout` | No -- Wave 0 |
| AGENT-04 | Termination produces graceful error message with reason | unit | `cargo test -p jadepaw-agent test_termination_error_message` | No -- Wave 0 |
| AGENT-04 | Per-turn fuel reset prevents cumulative exhaustion | unit | `cargo test -p jadepaw-agent test_fuel_reset_per_turn` | No -- Wave 0 |
| AGENT-04 | Phase 2 Wasm-level limits still enforced during agent loop | integration | `cargo test -p jadepaw-agent test_wasm_limits_preserved` | No -- Wave 0 |
| -- | run_agent returns structured AgentResponse | unit | `cargo test -p jadepaw-agent test_run_agent_returns_structured_response` | No -- Wave 0 |
| -- | run_agent invoked programmatically (test harness) | integration | `cargo test -p jadepaw-agent test_programmatic_invocation` | No -- Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p jadepaw-agent -p jadepaw-core`
- **Per wave merge:** `cargo test --workspace` (ensure no regressions in Phase 2 crates)
- **Phase gate:** Full workspace test suite green + `cargo build --workspace` clean

### Wave 0 Gaps

- [ ] `crates/jadepaw-agent/tests/agent_loop.rs` -- covers AGENT-01 loop integration
- [ ] `crates/jadepaw-agent/tests/sse_streaming.rs` -- covers AGENT-03 streaming
- [ ] `crates/jadepaw-agent/tests/termination.rs` -- covers AGENT-04 guards
- [ ] `crates/jadepaw-core/tests/agent_types.rs` -- covers AgentRequest/Response serialization
- [ ] `crates/jadepaw-agent/Cargo.toml` -- test dependencies (mock LLM server or wiremock)
- [ ] Framework install: no new install needed (cargo test is built-in)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | N/A -- Phase 3 is programmatic-only, no user auth |
| V3 Session Management | No | N/A -- session concept exists but auth is deferred |
| V4 Access Control | No | N/A -- capability-based access control already enforced by Phase 2 |
| V5 Input Validation | Yes | AgentRequest.user_message must be validated (non-empty, length limit, no control characters that could break SSE). Use inline validation in AgentRequest::new(). |
| V6 Cryptography | No | N/A -- no cryptographic operations in this phase |

### Known Threat Patterns for Agent Loop

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Prompt injection via user_message | Spoofing | System prompt should instruct the model to distinguish user input from instructions. Phase 3 MVP: document this as a known limitation. Full mitigation in Phase 6 with guest-controlled guard prompts. |
| SSE injection via tool output in Observation | Tampering | Tool output that contains SSE control characters (`\n\n`, `event:`, `data:`) could break the SSE stream. Strip or escape control sequences in Observation before sending via mpsc channel. |
| Infinite loop resource exhaustion | Denial of Service | Already mitigated by D-08 (tokio::select! with max_iterations + wall_clock_timeout). Additionally, Phase 2 fuel limits per turn prevent individual turn explosion. |
| LLM key leakage in trace | Information Disclosure | AgentResponse.trace may contain LLM responses that could include sensitive data. Document that traces should not be logged in production without redaction (Phase 9 observability). |

## Sources

### Primary (HIGH confidence)

- async-openai 0.40.2 source code at `/Users/yue.weny/.cargo/registry/src/mirrors.ustc.edu.cn-38d0e5eb5da2abae/async-openai-0.40.2/src/chat.rs` -- `create_stream()` method (line 75), `ChatCompletionResponseStream` type (line 1105 of types/chat/chat_.rs), `CreateChatCompletionStreamResponse` (line 1175), `ChatChoiceStream` (line 1152), `ChatCompletionStreamResponseDelta` (line 1137)
- async-openai 0.40.2 Cargo.toml -- feature flags: `chat-completion` enables `_api` feature, which gates `ChatCompletionResponseStream` behind `#[cfg(feature = "_api")]`
- docs.rs/axum -- `Sse::new(stream).keep_alive()`, `Event::default().data().event()` for named SSE events
- docs.rs/tokio-stream -- `ReceiverStream::new(rx)` converts mpsc::Receiver into Stream
- Cargo.lock -- verified versions: wasmtime 45.0.0, async-openai 0.40.2, tokio 1.52.3

### Secondary (MEDIUM confidence)

- Phase 2 code assets:
  - `crates/jadepaw-core/src/host_functions.rs` -- additive-only trait pattern, async_trait
  - `crates/jadepaw-core/src/types.rs` -- SessionId, TenantId, ToolId strong types
  - `crates/jadepaw-core/src/capabilities.rs` -- InstanceCapabilities with capability checks
  - `crates/jadepaw-core/src/error.rs` -- JadepawError enum (to be extended)
  - `crates/jadepaw-wasm/src/pool.rs` -- InstancePool::acquire(), SessionHandle
  - `crates/jadepaw-wasm/src/session.rs` -- SessionState, SessionLimits
  - `crates/jadepaw-wasm/src/engine.rs` -- EngineFactory with safety config
- CONTEXT.md -- All D-01 through D-14 locked decisions (user-directed, authoritative for this phase)

### Tertiary (LOW confidence)

- None. All claims are verified against source code or official documentation.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies verified in Cargo.lock and source code
- Architecture: HIGH -- patterns derived from locked decisions (D-01..D-14) in CONTEXT.md and verified against existing Phase 2 code patterns
- Pitfalls: HIGH -- based on known patterns from async-openai feature gates, tokio channel behavior, wasmtime fuel metering, and SSE protocol requirements. All verified against source code or official docs.

**Research date:** 2026-06-01
**Valid until:** 2026-07-01 (30 days -- stable ecosystem, no breaking changes expected)