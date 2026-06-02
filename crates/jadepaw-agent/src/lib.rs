//! # jadepaw-agent
//!
//! Agent runtime: ReAct execution loop with real-time LLM streaming via
//! async-openai. SSE events are relayed through a tokio mpsc channel for
//! Phase 7 HTMX frontend integration.
//!
//! ## What lives here
//!
//! - Top-level session orchestrator managing loop lifecycle
//! - ReAct step executor: think -> tool -> observe -> decide
//! - LLM integration via async-openai with streaming token relay
//! - SSE event mapping per D-14 (thought, action, observation, error, done)
//! - Termination protection (iteration limit, wall-clock timeout)
//!
//! ## What does NOT live here
//!
//! - Wasm instance pool or engine management (see jadepaw-wasm)
//! - HTTP/WS transport or session affinity (see jadepaw-gateway)
//! - Core data types (see jadepaw-core)
//! - Skill format or compilation (see jadepaw-skill)
//! - Tool registry with MCP-compatible protocol adapter (Phase 4)

pub mod guard;
pub mod llm;
pub mod r#loop;
pub mod stream;

use std::convert::Infallible;
use std::env::temp_dir;
use std::sync::Arc;

use async_openai::Client;
use async_openai::config::Config;
use axum::response::sse::Event;
use futures::stream::Stream;
use jadepaw_core::{AgentRequest, AgentResponse, JadepawError, ReActStep};
use jadepaw_wasm::{InstancePool, SessionState};

/// Run an agent session from request to response with real-time SSE streaming.
///
/// This is the primary entry point (D-13). It composes:
/// 1. Session acquisition from the instance pool
/// 2. SSE channel creation via `stream::create_sse_channel()`
/// 3. The ReAct loop (`react_loop`) with real async-openai LLM calls
/// 4. Termination protection (`run_with_guard`)
///
/// The result is a tuple of `(AgentResponse, SSE stream)`. The `AgentResponse`
/// contains the structured response (session_id, final_answer, trace), and the
/// SSE stream carries real-time token events that Phase 7's HTMX frontend can
/// consume directly.
///
/// The system prompt defaults to `llm::REACT_SYSTEM_PROMPT` unless overridden.
///
/// # Errors
///
/// Returns an error if:
/// - The session cannot be acquired from the pool
/// - The agent loop fails (LLM errors, channel drops)
/// - A termination guard fires (iteration limit, wall-clock timeout)
pub async fn run_agent(
    req: AgentRequest,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
) -> core::result::Result<(AgentResponse, impl Stream<Item = core::result::Result<Event, Infallible>>), JadepawError>
{
    let session_id = req.session_id;
    let user_message = req.user_message.clone();
    let context = req.context.as_deref();

    // Create session state with a temporary sandbox root
    let state = SessionState::with_defaults(temp_dir());

    // Acquire a session from the pool
    let mut handle = pool
        .acquire(session_id, state)
        .await
        .map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("failed to acquire session: {}", e),
                    turn: 0,
                },
            )
        })?;

    // Create SSE channel for real-time step streaming
    let (tx, sse_stream) = stream::create_sse_channel();

    let guard_config = guard::GuardConfig::default();
    let system_prompt = llm::REACT_SYSTEM_PROMPT;

    // Run the agent loop under termination protection
    let trace = guard::run_with_guard(&guard_config, || {
        r#loop::react_loop(
            &guard_config,
            &mut handle,
            &llm_client,
            model,
            system_prompt,
            &user_message,
            context,
            &tx,
        )
    })
    .await;

    // Drop the sender so the SSE stream knows we're done.
    // This MUST execute before any error propagation below — if run_with_guard
    // returned an Err, the SSE stream still needs the channel-close signal so
    // downstream consumers don't hang indefinitely waiting for events.
    drop(tx);

    let trace = trace?;

    // Extract the final answer from the trace.
    // If no Finished step is present (e.g., agent was terminated by guard),
    // return an error rather than silently producing an empty answer.
    let final_answer = trace
        .iter()
        .rev()
        .find_map(|step| match step {
            ReActStep::Finished { answer } => Some(answer.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: "agent completed without producing a final answer".to_string(),
                    turn: 0,
                },
            )
        })?;

    Ok((
        AgentResponse {
            session_id,
            final_answer,
            trace,
        },
        sse_stream,
    ))
}

// Re-export key public types
pub use guard::{run_with_guard, GuardConfig};
pub use llm::{
    REACT_SYSTEM_PROMPT, build_initial_messages, parse_next_action, stream_llm_response,
};
pub use r#loop::react_loop;
pub use stream::create_sse_channel;