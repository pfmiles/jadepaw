//! ReAct loop orchestrator.
//!
//! Implements the think-act-observe cycle (D-01). Runs on the host side.
//! Each turn resets the Wasm fuel budget, accumulates the full LLM response,
//! and emits a single `ReActStep::Thought` event per turn via an mpsc channel.
//! The loop produces a complete execution trace of `ReActStep` items.
//!
//! # Design (D-01, D-10, D-07)
//!
//! - ReAct loop skeleton: think -> act -> observe -> repeat
//! - Per-turn fuel reset at 1_000_000 units (Pitfall 3 prevention)
//! - Uses real async-openai `Client<Box<dyn Config>>` with streaming
//! - LLM response is parsed for ACTION / FINAL ANSWER directives
//! - Full LLM response is emitted as a single `ReActStep::Thought` per turn

use std::fmt;

use anyhow::Context;
use async_openai::{
    types::chat::{ChatCompletionRequestMessage, ChatCompletionRequestUserMessage},
    Client,
    config::Config,
};
use jadepaw_core::ReActStep;
use jadepaw_wasm::SessionHandle;
use tokio::sync::mpsc;

use crate::guard::GuardConfig;
use crate::llm::{self, LlmDirective};
use crate::tool_registry::ToolRegistry;

/// Structured error kind for the ReAct loop.
///
/// Used by `run_with_guard` to classify errors without fragile string matching.
/// Each variant carries enough context for the guard to produce the correct
/// `AgentTerminationReason`.
#[derive(Debug)]
pub(crate) enum LoopErrorKind {
    /// The iteration limit was exhausted.
    MaxIterations {
        /// The current iteration count at termination.
        iter: u32,
        /// The configured maximum.
        max: u32,
    },
    /// An LLM call failed (infrastructure error, not a Wasm trap).
    LlmFailure {
        /// The turn number on which the failure occurred.
        turn: u32,
        /// The underlying error.
        source: anyhow::Error,
    },
    /// The Wasm session store failed (fuel reset, memory access, etc.).
    StoreFailure {
        /// The turn number on which the failure occurred.
        turn: u32,
        /// The underlying error.
        source: anyhow::Error,
    },
    /// The output SSE channel was closed (client disconnected).
    ChannelClosed {
        /// The turn number on which the channel was detected closed.
        turn: u32,
    },
}

impl fmt::Display for LoopErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxIterations { iter, max } => {
                write!(
                    f,
                    "max iterations ({max}) reached without completion (attempted {iter})"
                )
            }
            Self::LlmFailure { turn, source } => {
                write!(f, "LLM call failed on turn {turn}: {source}")
            }
            Self::StoreFailure { turn, source } => {
                write!(f, "session store access failed on turn {turn}: {source}")
            }
            Self::ChannelClosed { turn } => {
                write!(f, "output channel closed on turn {turn}")
            }
        }
    }
}

impl std::error::Error for LoopErrorKind {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::LlmFailure { source, .. } | Self::StoreFailure { source, .. } => {
                Some(source.as_ref())
            }
            _ => None,
        }
    }
}

/// Create an `anyhow::Error` from a `LoopErrorKind`.
fn loop_error(kind: LoopErrorKind) -> anyhow::Error {
    anyhow::Error::new(kind)
}

/// Execute the ReAct loop for a single agent session.
///
/// Orchestrates think-act-observe cycles using real async-openai streaming.
/// Each turn:
/// 1. Resets the Wasm fuel budget on the session store (D-10 Pitfall 3)
/// 2. Calls `llm::stream_llm_response()` with the current message history
/// 3. Parses the LLM response with `llm::parse_next_action()`
/// 4. Based on the next action:
///    - `Finish` -> emits `ReActStep::Finished`, breaks the loop
///    - `Act` -> dispatches through `ToolRegistry::call_tool()`, emits
///      `ReActStep::Action` and `ReActStep::Observation` with real tool
///      output, appends tool result to LLM message history
///    - `ContinueThinking` -> appends the response to history, continues
/// 5. The full LLM response is emitted as a single `ReActStep::Thought` event via `tx`
///
/// # Errors
///
/// Returns an error if:
/// - The LLM call fails
/// - The session store fuel reset fails
/// - All iterations are exhausted without a finish signal
/// - The output channel is closed
pub async fn react_loop(
    guard_config: &GuardConfig,
    session: &mut SessionHandle,
    llm_client: &Client<Box<dyn Config>>,
    model: &str,
    system_prompt: &str,
    user_message: &str,
    context: Option<&str>,
    tx: &mpsc::Sender<ReActStep>,
    tool_registry: &ToolRegistry,
) -> anyhow::Result<Vec<ReActStep>> {
    let mut trace: Vec<ReActStep> = Vec::new();

    // Build initial messages
    let mut messages: Vec<ChatCompletionRequestMessage> =
        llm::build_initial_messages(system_prompt, user_message, context);

    // TODO(WR-04): Implement message windowing to prevent unbounded context
    // growth. Each ReAct turn adds 2 messages (observation + assistant response),
    // and at max_iterations=20 the history reaches ~42 messages. Future work:
    // - Sliding window: keep system + user + last N turns
    // - Summarization: periodically condense older turns into a summary
    // - Token counting: trim when approaching the model's context limit

    for turn in 0..guard_config.max_iterations {
        // Per-turn fuel reset (D-10 Pitfall 3)
        session
            .store_mut()
            .set_fuel(1_000_000)
            .map_err(|e| {
                loop_error(LoopErrorKind::StoreFailure {
                    turn,
                    source: anyhow::anyhow!("failed to set fuel on session store: {}", e),
                })
            })?;

        // Stream LLM response — accumulates full text without per-token events
        let full_response = llm::stream_llm_response(
            llm_client,
            messages.clone(),
            model,
            tx,
        )
        .await
        .with_context(|| format!("LLM call failed |turn={}|", turn))
            .map_err(|e| {
                loop_error(LoopErrorKind::LlmFailure {
                    turn,
                    source: e,
                })
            })?;

        // Emit a single Thought event with the complete LLM response
        let thought = ReActStep::Thought {
            content: full_response.clone(),
        };
        if tx.send(thought.clone()).await.is_err() {
            return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
        }
        // Push thought to trace so all branches (Finish, Act, ContinueThinking)
        // preserve the full reasoning context in the structured response.
        trace.push(thought);

        // Parse the response for next action
        let action = llm::parse_next_action(&full_response);

        match action {
            LlmDirective::Finish { thought: _, answer } => {
                // Emit finished event.
                // NOTE: The `thought` field from LlmDirective::Finish is
                // intentionally NOT stored in ReActStep::Finished. The
                // final reasoning context is already present in the trace
                // via the ReActStep::Thought pushed at the start of this
                // turn (line 159). Adding it redundantly to Finished would
                // bloat the trace without adding information.
                let finished = ReActStep::Finished {
                    answer: answer.clone(),
                };
                // Push to trace BEFORE sending to tx: if the SSE consumer
                // processes the done event and disconnects, the channel
                // close is immediate. The local trace must be complete
                // before the external notification goes out so the
                // upstream caller always finds the Finished step.
                trace.push(finished.clone());
                if tx.send(finished).await.is_err() {
                    return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
                }
                return Ok(trace);
            }
            LlmDirective::Act { thought: _, tool, args } => {
                // Emit action step.
                // Attempt JSON parse of args; on failure, log a warning
                // and fall back to wrapping as a plain string value so
                // downstream consumers can still inspect the raw args.
                let parsed_args = match serde_json::from_str(&args) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            turn = turn,
                            tool = %tool,
                            raw_args = %args,
                            error = %e,
                            "failed to parse tool args as JSON, storing as raw string"
                        );
                        serde_json::Value::String(args.clone())
                    }
                };
                let action_step = ReActStep::Action {
                    tool: tool.clone(),
                    args: parsed_args.clone(),
                };
                // Push before send (consistent with WR-02 Finish branch fix):
                // the local trace must be complete before external notification.
                trace.push(action_step.clone());
                if tx.send(action_step).await.is_err() {
                    return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
                }

                // Phase 4: dispatch through ToolRegistry (replaces placeholder)
                let tool_result = tool_registry.call_tool(&tool, parsed_args, session).await;
                let is_error = tool_result.is_error();
                let result_str = tool_result.to_observation_string();

                let observation = ReActStep::Observation {
                    result: result_str.clone(),
                    is_error,
                };
                trace.push(observation.clone());
                if tx.send(observation).await.is_err() {
                    return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
                }

                // Append tool result to LLM message history so the LLM can adapt
                let observation_msg: ChatCompletionRequestMessage =
                    ChatCompletionRequestUserMessage::from(
                        format!("Tool '{}' result:\n{}", tool, result_str),
                    )
                    .into();
                messages.push(observation_msg);

                // Append the assistant's response to message history
                let assistant_msg: ChatCompletionRequestMessage =
                    async_openai::types::chat::ChatCompletionRequestAssistantMessage::from(
                        full_response,
                    )
                    .into();
                messages.push(assistant_msg);
            }
            LlmDirective::ContinueThinking { thought: _ } => {
                // Append the response to history and continue
                let assistant_msg: ChatCompletionRequestMessage =
                    async_openai::types::chat::ChatCompletionRequestAssistantMessage::from(
                        full_response,
                    )
                    .into();
                messages.push(assistant_msg);
            }
        }
    }

    let _ = tx
        .send(ReActStep::Error {
            message: format!(
                "max iterations ({}) reached without completion",
                guard_config.max_iterations
            ),
            turn: guard_config.max_iterations.saturating_sub(1),
        })
        .await;
    return Err(loop_error(LoopErrorKind::MaxIterations {
        iter: guard_config.max_iterations,
        max: guard_config.max_iterations,
    }));
}