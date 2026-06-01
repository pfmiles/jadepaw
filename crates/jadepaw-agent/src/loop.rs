//! ReAct loop orchestrator.
//!
//! Implements the think-act-observe cycle (D-01). Runs on the host side.
//! Each turn resets the Wasm fuel budget and polls the LLM for the next
//! step. The loop produces a complete execution trace of `ReActStep` items
//! and streams them in real time via an mpsc channel.
//!
//! # Design (D-01, D-10)
//!
//! - ReAct loop skeleton: think -> act -> observe -> repeat
//! - Per-turn fuel reset at 1_000_000 units (Pitfall 3 prevention)
//! - `LlmProvider` trait allows mocking the LLM for testing
//! - Plan 02 replaces the mocked LLM with real async-openai streaming

use anyhow::Context;
use async_trait::async_trait;
use jadepaw_core::ReActStep;
use jadepaw_wasm::SessionHandle;
use tokio::sync::mpsc;

/// Configuration for a ReAct loop execution.
#[derive(Clone)]
pub struct LoopConfig {
    /// Maximum number of think-act-observe iterations.
    pub max_iterations: u32,
    /// The LLM model identifier (e.g., "gpt-4o").
    pub model: String,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            model: "gpt-4o".to_string(),
        }
    }
}

/// Mockable LLM provider interface.
///
/// Provides a single `chat` method that takes message history and returns
/// a response string. Used by the ReAct loop to obtain the next step. Plan 02
/// replaces this with real async-openai streaming integration.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send messages to the LLM and receive a response string.
    ///
    /// `messages` are the conversation history. The implementation may
    /// format them as system/user/assistant messages internally.
    async fn chat(&self, messages: &[String]) -> anyhow::Result<String>;
}

/// Execute the ReAct loop for a single agent session.
///
/// Orchestrates think-act-observe cycles. Each turn:
/// 1. Resets the Wasm fuel budget on the session store
/// 2. Calls the LLM with the message history to get a response
/// 3. Emits a `Thought` step via the channel and pushes it to the trace
/// 4. Emits a `Finished` step via the channel, pushes it to the trace, and breaks
///
/// # Errors
///
/// Returns an error if:
/// - The LLM call fails
/// - The session store fuel reset fails
/// - All iterations are exhausted without a finish signal
pub async fn react_loop(
    config: &LoopConfig,
    session: &mut SessionHandle,
    llm: &dyn LlmProvider,
    tx: &mpsc::Sender<ReActStep>,
) -> anyhow::Result<Vec<ReActStep>> {
    let mut trace: Vec<ReActStep> = Vec::new();
    let mut history: Vec<String> = Vec::new();

    // Initialize history with a simple prompt to elicit a response
    history.push("You are a helpful AI assistant. Respond with your thoughts and a final answer.".to_string());

    for turn in 0..config.max_iterations {
        // Per-turn fuel reset (D-10 Pitfall 3)
        session
            .store_mut()
            .set_fuel(1_000_000)
            .map_err(|e| anyhow::anyhow!("failed to set fuel on session store: {}", e))?;

        // Call LLM for current turn
        let response = llm
            .chat(&history)
            .await
            .with_context(|| format!("LLM call failed on turn {}", turn))?;

        // Emit and record Thought step
        let thought = ReActStep::Thought {
            content: response.clone(),
        };
        if tx.send(thought.clone()).await.is_err() {
            // Receiver dropped — caller is no longer listening, stop gracefully
            anyhow::bail!("output channel closed on turn {}", turn);
        }
        trace.push(thought);

        // Emit and record Finished step with the LLM's response as answer
        let finished = ReActStep::Finished {
            answer: response,
        };
        if tx.send(finished.clone()).await.is_err() {
            anyhow::bail!("output channel closed on turn {}", turn);
        }
        trace.push(finished);

        break;
    }

    // If we exhausted all iterations without breaking, signal termination
    if trace.len() >= 2 && matches!(trace.last(), Some(ReActStep::Finished { .. })) {
        // Normal completion — trace ends with Finished
        Ok(trace)
    } else {
        anyhow::bail!(
            "max iterations ({}) reached without completion",
            config.max_iterations
        )
    }
}