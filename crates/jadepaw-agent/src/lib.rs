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
//! - Tool registry with MCP-compatible protocol adapter (now in `tool_registry` module)

pub mod guard;
pub mod llm;
pub mod r#loop;
pub mod stream;
pub mod tool_registry;
pub mod window;

use std::convert::Infallible;
use std::env::temp_dir;
use std::sync::Arc;

use async_openai::Client;
use async_openai::config::Config;
use axum::response::sse::Event;
use futures::stream::Stream;
use jadepaw_core::{AgentRequest, AgentResponse, JadepawError, ReActStep, SessionId, TenantId};
use jadepaw_skill::SkillManager;
use jadepaw_wasm::{InstancePool, SessionState};
use jadepaw_db::{self, SessionRepository, SessionStatus};

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
/// When `tool_registry` is provided, available tool descriptions are injected
/// into the system prompt via `build_system_prompt_with_tools()`. Pass `None`
/// to run the agent without tools (backward compatible with Phase 3 behavior).
///
/// # Errors
///
/// Returns an error if:
/// - The session cannot be acquired from the pool
/// - The agent loop fails (LLM errors, channel drops)
/// - A termination guard fires (iteration limit, wall-clock timeout)
pub async fn run_agent(
    req: AgentRequest,
    tenant_id: TenantId,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    tool_registry: Option<Arc<ToolRegistry>>,
    skill_manager: Option<Arc<SkillManager>>,
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
    let registry = tool_registry.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
    let system_prompt = llm::REACT_SYSTEM_PROMPT;

    // Build skill-aware augmented system prompt (D-03).
    let tool_definitions = registry.list_tools();
    let augmented_prompt = if let Some(ref sm) = skill_manager {
        if sm.has_active_skills(tenant_id) {
            let (skill_block, _tool_names) = sm.merge_active(tenant_id);
            llm::build_skill_augmented_prompt(system_prompt, &skill_block, &tool_definitions)
        } else if tool_definitions.is_empty() {
            system_prompt.to_string()
        } else {
            llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
        }
    } else if tool_definitions.is_empty() {
        system_prompt.to_string()
    } else {
        llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
    };

    // Run the agent loop under termination protection.
    // Fresh sessions: no session_repo, empty pre-existing state, start at turn 0.
    let session_created_at = chrono::Utc::now();
    let trace = guard::run_with_guard(&guard_config, || {
        r#loop::react_loop(
            &guard_config,
            &mut handle,
            &llm_client,
            model,
            &augmented_prompt,
            &user_message,
            context,
            &tx,
            &registry,
            skill_manager.clone(),
            None,                       // session_repo: no persistence for fresh sessions
            session_id,
            tenant_id,
            vec![],                    // pre_existing_messages
            vec![],                    // pre_existing_trace
            0,                         // elapsed_accumulator_ms
            0,                         // start_turn
            session_created_at,
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

/// Resume a paused session from a database snapshot.
///
/// Loads the snapshot, reconstructs the ReAct loop state, acquires a fresh
/// Wasm Store from InstancePool (D-06a: Stores are NOT serialized), and
/// continues execution from the next turn.
///
/// # Errors
///
/// Returns an error if:
/// - The session cannot be found in the database
/// - The snapshot cannot be deserialized
/// - The fresh Wasm Store cannot be acquired
/// - The agent loop fails
pub async fn resume_session(
    session_id: SessionId,
    tenant_id: TenantId,
    pool: Arc<InstancePool>,
    llm_client: Client<Box<dyn Config>>,
    model: &str,
    repo: &dyn SessionRepository,
    tool_registry: Option<Arc<ToolRegistry>>,
    skill_manager: Option<Arc<SkillManager>>,
) -> core::result::Result<
    (
        AgentResponse,
        impl Stream<Item = core::result::Result<Event, Infallible>>,
    ),
    JadepawError,
> {
    // 1. Load snapshot from DB
    let snapshot = repo
        .load(session_id, tenant_id)
        .await
        .map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("failed to load session: {}", e),
                    turn: 0,
                },
            )
        })?
        .ok_or_else(|| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("session not found: {}", session_id),
                    turn: 0,
                },
            )
        })?;

    // 2. Deserialize conversational state from JSON blobs
    let messages: Vec<async_openai::types::chat::ChatCompletionRequestMessage> =
        serde_json::from_str(&snapshot.messages_json).map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("failed to deserialize messages: {}", e),
                    turn: 0,
                },
            )
        })?;

    let pre_trace: Vec<ReActStep> =
        serde_json::from_str(&snapshot.trace_json).map_err(|e| {
            JadepawError::agent_terminated(
                jadepaw_core::AgentTerminationReason::InfrastructureError {
                    reason: format!("failed to deserialize trace: {}", e),
                    turn: 0,
                },
            )
        })?;

    // 3. Defensive check: if pre_existing_messages is empty (snapshot
    // corruption, deserialization producing zero messages, manual DB
    // manipulation), return an infrastructure error rather than passing
    // an empty message list to react_loop, which would produce degenerate
    // output from build_initial_messages.
    if messages.is_empty() {
        return Err(JadepawError::agent_terminated(
            jadepaw_core::AgentTerminationReason::InfrastructureError {
                reason: "corrupted session snapshot: pre_existing_messages is empty".to_string(),
                turn: 0,
            },
        ));
    }

    // 4. Deserialize guard config, fall back to default on parse failure
    let guard_config: guard::GuardConfig =
        serde_json::from_str(&snapshot.guard_config_json).unwrap_or_default();

    // 5. Update status to Running before entering the loop.
    // Log failures at error level — an incorrect DB status means crash
    // recovery (mark_running_as_paused) won't detect this session.
    if let Err(e) = repo
        .update_status(session_id, tenant_id, SessionStatus::Running)
        .await
    {
        tracing::error!(
            error = %e,
            session_id = %session_id,
            tenant_id = %tenant_id,
            "failed to update session status to running; crash recovery may miss this session"
        );
    }

    // 6. Acquire fresh Wasm Store — D-06a: Stores are NOT serialized
    let state = SessionState::with_defaults(temp_dir());
    let mut handle = pool.acquire(session_id, state).await.map_err(|e| {
        JadepawError::agent_terminated(
            jadepaw_core::AgentTerminationReason::InfrastructureError {
                reason: format!("failed to acquire session for resume: {}", e),
                turn: 0,
            },
        )
    })?;

    // 7. Create SSE channel
    let (tx, sse_stream) = stream::create_sse_channel();

    let registry = tool_registry.unwrap_or_else(|| Arc::new(ToolRegistry::new()));
    let system_prompt = llm::REACT_SYSTEM_PROMPT;

    // Build skill-aware augmented system prompt (D-03).
    let tool_definitions = registry.list_tools();
    let augmented_prompt = if let Some(ref sm) = skill_manager {
        if sm.has_active_skills(tenant_id) {
            let (skill_block, _tool_names) = sm.merge_active(tenant_id);
            llm::build_skill_augmented_prompt(system_prompt, &skill_block, &tool_definitions)
        } else if tool_definitions.is_empty() {
            system_prompt.to_string()
        } else {
            llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
        }
    } else if tool_definitions.is_empty() {
        system_prompt.to_string()
    } else {
        llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
    };

    // 8. Run react_loop with pre-existing state, persisting at turn boundaries
    let trace = guard::run_with_guard(&guard_config, || {
        r#loop::react_loop(
            &guard_config,
            &mut handle,
            &llm_client,
            model,
            &augmented_prompt,
            "", // user_message: not used when pre_existing_messages is not empty
            None, // context: already part of pre_existing_messages
            &tx,
            &registry,
            skill_manager.clone(),
            Some(repo),                         // session_repo
            session_id,
            tenant_id,
            messages,                           // pre_existing_messages
            pre_trace,                          // pre_existing_trace
            snapshot.elapsed_ms,                // elapsed_accumulator_ms
            snapshot.iteration_count,           // start_turn
            snapshot.created_at,                // session_created_at
        )
    })
    .await;

    // Drop the sender so the SSE stream knows we're done.
    drop(tx);

    let trace = trace?;

    // 9. Update status to Ended. Log failures at error level — if this
    // update fails, the session remains "running" in the DB and would be
    // picked up by crash recovery on next startup (mark_running_as_paused),
    // creating a zombie session.
    if let Err(e) = repo
        .update_status(session_id, tenant_id, SessionStatus::Ended)
        .await
    {
        tracing::error!(
            error = %e,
            session_id = %session_id,
            tenant_id = %tenant_id,
            "failed to update session status to ended; session may appear as zombie in crash recovery"
        );
    }

    // Extract the final answer
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
    REACT_SYSTEM_PROMPT, build_initial_messages, build_skill_augmented_prompt,
    build_system_prompt_with_tools, parse_next_action, stream_llm_response,
};
pub use r#loop::react_loop;
pub use stream::create_sse_channel;
pub use tool_registry::ToolRegistry;
pub use window::{compress_context, count_tokens, should_compress};