//! Agent request/response types and ReAct loop execution primitives.
//!
//! Defines the core data structures for the agent runtime: `AgentRequest` (what
//! callers provide), `AgentResponse` (what the agent returns), `ReActStep`
//! (individual think-act-observe steps), and `AgentTerminationReason` (why an
//! agent run ended).
//!
//! # Design (D-12)
//!
//! - All types live in `jadepaw-core` with serde `Serialize`/`Deserialize`
//! - No wasmtime dependency — usable by jadepaw-agent and other crates without
//!   pulling in the Wasm runtime
//! - `AgentTerminationReason` uses u64 for time values instead of `Duration`
//!   to maintain `PartialEq`/`Eq` derive compatibility

use crate::types::SessionId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A request to run an agent session.
///
/// Contains the session identifier, the user's input message, and optional
/// context (e.g., system prompt, skill instructions, prior conversation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// The user's natural language message.
    pub user_message: String,
    /// Optional context string (system prompts, skill instructions, etc.).
    pub context: Option<String>,
}

impl Default for AgentRequest {
    fn default() -> Self {
        Self {
            session_id: SessionId::new(),
            user_message: String::new(),
            context: None,
        }
    }
}

/// The result of a completed agent run.
///
/// Contains the session identifier, the final answer produced by the agent,
/// and the full execution trace of ReAct steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Unique session identifier (matches the request).
    pub session_id: SessionId,
    /// The agent's final answer to the user.
    pub final_answer: String,
    /// The complete execution trace (all think-act-observe steps).
    pub trace: Vec<ReActStep>,
}

/// A single step in the ReAct (Reasoning + Acting) execution loop.
///
/// Each iteration of the agent loop produces one or more steps. The trace
/// is built up over the course of a session and returned in `AgentResponse`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReActStep {
    /// A reasoning step — the agent thinks about what to do next.
    Thought {
        /// The agent's reasoning content.
        content: String,
    },
    /// An action step — the agent invokes a tool.
    Action {
        /// The name of the tool being invoked.
        tool: String,
        /// JSON-encoded arguments passed to the tool.
        args: serde_json::Value,
    },
    /// An observation step — the result of a tool invocation.
    Observation {
        /// The result returned by the tool.
        result: String,
    },
    /// An error occurred during a turn.
    Error {
        /// Human-readable error message.
        message: String,
        /// The turn number (0-indexed) on which the error occurred.
        turn: u32,
    },
    /// The agent has finished and produced a final answer.
    Finished {
        /// The final answer text.
        answer: String,
    },
}

/// The reason an agent run was terminated.
///
/// Agents may be terminated for several reasons: iteration limits, wall-clock
/// timeouts, or Wasm guest traps. This enum captures the specific reason in a
/// machine-readable form that the caller can inspect programmatically.
///
/// Note: time values use u64 (seconds) instead of `std::time::Duration` to
/// maintain `PartialEq`/`Eq` derive compatibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTerminationReason {
    /// The agent exceeded the maximum number of ReAct iterations.
    MaxIterationsReached {
        /// The number of iterations that were attempted.
        iter: u32,
        /// The configured maximum.
        max: u32,
    },
    /// The agent exceeded the wall-clock time limit.
    WallClockTimeout {
        /// The elapsed time in milliseconds.
        elapsed_ms: u64,
        /// The configured maximum in milliseconds.
        max_ms: u64,
    },
    /// The Wasm guest trapped during execution.
    WasmTrap {
        /// Human-readable trap reason.
        reason: String,
        /// The turn number (0-indexed) on which the trap occurred.
        turn: u32,
    },
    /// An infrastructure error that is not a Wasm trap.
    ///
    /// Used for LLM API failures, channel closures (client disconnects),
    /// pool acquisition failures, and other host-side errors that are
    /// semantically distinct from guest sandbox violations.
    InfrastructureError {
        /// Human-readable error context.
        reason: String,
        /// The turn number (0-indexed) on which the error occurred.
        turn: u32,
    },
}

impl fmt::Display for AgentTerminationReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxIterationsReached { iter, max } => {
                write!(
                    f,
                    "agent exceeded max iterations: attempted {} but limit is {}",
                    iter, max
                )
            }
            Self::WallClockTimeout { elapsed_ms, max_ms } => {
                write!(
                    f,
                    "agent timed out after {:.1}s (limit: {:.1}s)",
                    *elapsed_ms as f64 / 1000.0,
                    *max_ms as f64 / 1000.0
                )
            }
            Self::WasmTrap { reason, turn } => {
                write!(
                    f,
                    "wasm guest trapped on turn {}: {}",
                    turn, reason
                )
            }
            Self::InfrastructureError { reason, turn } => {
                write!(
                    f,
                    "infrastructure error on turn {}: {}",
                    turn, reason
                )
            }
        }
    }
}