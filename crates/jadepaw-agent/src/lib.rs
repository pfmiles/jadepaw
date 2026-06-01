//! # jadepaw-agent
//!
//! Agent runtime: hybrid planning loop (coarse-grained plan + ReAct execution),
//! tool management, memory management, and LLM client integration.
//!
//! ## What lives here
//!
//! - Top-level session orchestrator managing loop lifecycle
//! - LLM-driven plan generation with deviation-triggered re-planning
//! - ReAct step executor: think -> tool -> observe -> decide
//! - Tool registry with MCP-compatible protocol adapter
//! - Short-term memory (window compression) and long-term memory (vector DB)
//! - `LlmClient` trait: unified LLM backend abstraction for provider decoupling
//!   (OpenAI-compatible via async-openai, Anthropic via native API — Phase 3)
//!
//! ## What does NOT live here
//!
//! - Wasm instance pool or engine management (see jadepaw-wasm)
//! - HTTP/WS transport or session affinity (see jadepaw-gateway)
//! - Core data types (see jadepaw-core)
//! - Skill format or compilation (see jadepaw-skill)

pub mod guard;
pub mod r#loop;

use std::env::temp_dir;
use std::sync::Arc;

use jadepaw_core::{AgentRequest, AgentResponse, JadepawError, ReActStep};
use jadepaw_wasm::{InstancePool, SessionState};
use tokio::sync::mpsc;

/// Run an agent session from request to response.
///
/// This is the primary entry point (D-13). It composes:
/// 1. Session acquisition from the instance pool
/// 2. The ReAct loop (`react_loop`)
/// 3. Termination protection (`run_with_guard`)
///
/// The result is a structured `AgentResponse` with the session identifier,
/// final answer, and complete execution trace.
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
    llm: &dyn self::LlmProvider,
) -> core::result::Result<AgentResponse, JadepawError> {
    let session_id = req.session_id;

    // Create session state with a temporary sandbox root
    let state = SessionState::with_defaults(temp_dir());

    // Acquire a session from the pool
    let mut handle = pool
        .acquire(session_id, state)
        .await
        .map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::WasmTrap {
                    reason: format!("failed to acquire session: {}", e),
                    turn: 0,
                },
            )
        })?;

    // Create output channel for real-time step streaming
    let (tx, _rx) = mpsc::channel::<ReActStep>(256);

    let loop_config = r#loop::LoopConfig::default();
    let guard_config = guard::GuardConfig::default();

    // Run the agent loop under termination protection
    let trace = guard::run_with_guard(guard_config, || {
        r#loop::react_loop(&loop_config, &mut handle, llm, &tx)
    })
    .await?;

    // Extract the final answer from the trace
    let final_answer = trace
        .iter()
        .rev()
        .find_map(|step| match step {
            ReActStep::Finished { answer } => Some(answer.clone()),
            _ => None,
        })
        .unwrap_or_default();

    Ok(AgentResponse {
        session_id,
        final_answer,
        trace,
    })
}

// Re-export key public types
pub use guard::{run_with_guard, GuardConfig};
pub use r#loop::{react_loop, LlmProvider, LoopConfig};
