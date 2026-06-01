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

use anyhow::Context;
use async_openai::{types::chat::ChatCompletionRequestMessage, Client, config::Config};
use jadepaw_core::ReActStep;
use jadepaw_wasm::SessionHandle;
use tokio::sync::mpsc;

use crate::llm::{self, LlmDirective};

/// Configuration for a ReAct loop execution.
#[derive(Clone)]
pub struct LoopConfig {
    /// Maximum number of think-act-observe iterations.
    pub max_iterations: u32,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
        }
    }
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
///    - `Act` -> emits `ReActStep::Action`, then a placeholder `Observation`
///      (full tool execution is Phase 4)
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
    config: &LoopConfig,
    session: &mut SessionHandle,
    llm_client: &Client<Box<dyn Config>>,
    model: &str,
    system_prompt: &str,
    user_message: &str,
    context: Option<&str>,
    tx: &mpsc::Sender<ReActStep>,
) -> anyhow::Result<Vec<ReActStep>> {
    let mut trace: Vec<ReActStep> = Vec::new();

    // Build initial messages
    let mut messages: Vec<ChatCompletionRequestMessage> =
        llm::build_initial_messages(system_prompt, user_message, context);

    for turn in 0..config.max_iterations {
        // Per-turn fuel reset (D-10 Pitfall 3)
        session
            .store_mut()
            .set_fuel(1_000_000)
            .map_err(|e| {
                anyhow::anyhow!("failed to set fuel on session store: {}", e)
            })?;

        // Stream LLM response — accumulates full text without per-token events
        let full_response = llm::stream_llm_response(
            llm_client,
            messages.clone(),
            model,
            tx,
        )
        .await
        .with_context(|| format!("LLM call failed on turn {}", turn))?;

        // Emit a single Thought event with the complete LLM response
        let thought = ReActStep::Thought {
            content: full_response.clone(),
        };
        if tx.send(thought).await.is_err() {
            anyhow::bail!("output channel closed on turn {}", turn);
        }

        // Parse the response for next action
        let action = llm::parse_next_action(&full_response);

        match action {
            LlmDirective::Finish { thought: _, answer } => {
                // Emit finished event
                let finished = ReActStep::Finished {
                    answer: answer.clone(),
                };
                if tx.send(finished.clone()).await.is_err() {
                    anyhow::bail!("output channel closed on turn {}", turn);
                }
                trace.push(finished);
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
                    args: parsed_args,
                };
                if tx.send(action_step.clone()).await.is_err() {
                    anyhow::bail!("output channel closed on turn {}", turn);
                }
                trace.push(action_step);

                // Emit placeholder observation (full tool execution in Phase 4)
                let observation = ReActStep::Observation {
                    result: format!(
                        "Tool '{}' called with args '{}'. Full tool execution is coming in Phase 4.",
                        tool, args
                    ),
                };
                if tx.send(observation.clone()).await.is_err() {
                    anyhow::bail!("output channel closed on turn {}", turn);
                }
                trace.push(observation);

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

    anyhow::bail!(
        "max iterations ({}) reached without completion",
        config.max_iterations
    )
}