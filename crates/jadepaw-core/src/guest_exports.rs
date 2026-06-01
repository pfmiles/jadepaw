//! Guest export trait — the Wasm guest's decision-point interface.
//!
//! The `GuestExports` trait defines the interface that a guest Wasm module
//! can optionally implement to override host-side decision points in the ReAct
//! loop. When the guest does not export a particular function, the host falls
//! back to LLM-based defaults.
//!
//! # Design (D-03, D-04)
//!
//! - **Additive-only**: methods may be added, never removed. Breaking changes
//!   require a major version bump.
//! - **Optional by default**: every method returns `Option<...>` defaulting to
//!   `None`, signalling "host fallback".
//! - **No wasmtime dependency**: lives in `jadepaw-core` so the agent crate
//!   can reference it without pulling in the runtime.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A tool definition as exposed by the host to the guest.
///
/// Each tool has a name, a human-readable description, and a JSON Schema
/// describing the parameters it accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDef {
    /// The tool's name (e.g., "file_write", "http_request").
    pub name: String,
    /// A human-readable description of what the tool does.
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    pub parameters_schema: serde_json::Value,
}

/// The next action the agent should take, as decided by the guest.
///
/// Returned by `evaluate_step` to signal whether the agent should continue
/// thinking, invoke a tool, or finish with an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextAction {
    /// Continue thinking — the agent should reason further.
    ContinueThinking,
    /// Invoke a tool with the given name and arguments.
    Act {
        /// The tool to invoke.
        tool: String,
        /// JSON-encoded arguments for the tool.
        args: serde_json::Value,
    },
    /// Finish the session with a final answer.
    Finish {
        /// The final answer text.
        answer: String,
    },
}

/// The tool chosen by the guest from a list of available tools.
///
/// Returned by `select_tool` to signal which tool should be invoked, or
/// that no suitable tool is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    /// A specific tool was selected.
    Select {
        /// The name of the selected tool.
        tool_name: String,
    },
    /// No available tool matches the goal.
    NoneAvailable,
}

impl Default for ToolChoice {
    fn default() -> Self {
        Self::NoneAvailable
    }
}

/// The guest-side decision-point interface for the ReAct loop.
///
/// Each method corresponds to a decision point in the agent loop where the
/// guest Wasm module can override host-side LLM-based defaults. All methods
/// return `Option<...>` — `None` means "defer to host fallback".
///
/// # Additive-only policy
///
/// Methods may be added, never removed. CI must verify all implementors
/// cover every method.
#[async_trait]
pub trait GuestExports: Send + Sync {
    /// Evaluate the current thought and observation to decide the next action.
    ///
    /// Called after the agent produces a thought and an observation. The guest
    /// can inspect the content and decide whether to continue thinking, invoke
    /// a tool, or finish.
    ///
    /// Returns `None` to defer to the host's LLM-based fallback.
    async fn evaluate_step(
        &self,
        thought: String,
        observation: String,
    ) -> Option<NextAction> {
        let _ = (thought, observation);
        None
    }

    /// Select a tool from the available set for the given goal.
    ///
    /// Called when the agent decides to invoke a tool. The guest can inspect
    /// the goal and the available tools and choose the most appropriate one.
    ///
    /// Returns `None` to defer to the host's LLM-based fallback.
    async fn select_tool(
        &self,
        goal: String,
        available_tools: Vec<ToolDef>,
    ) -> Option<ToolChoice> {
        let _ = (goal, available_tools);
        None
    }

    /// Decide whether the agent should continue executing.
    ///
    /// Called at the end of each turn. The guest can inspect the turn count
    /// and a summary of the history to decide whether the agent should keep
    /// going or stop.
    ///
    /// Returns `None` to defer to the host's LLM-based fallback (i.e., the
    /// host decides based on its own criteria).
    async fn should_continue(
        &self,
        turn: u32,
        history_summary: String,
    ) -> Option<bool> {
        let _ = (turn, history_summary);
        None
    }
}