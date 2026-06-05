//! Termination guard — enforces iteration limits and wall-clock timeouts.
//!
//! The guard races the agent loop future against a wall-clock timeout using
//! `tokio::select!`. The iteration limit is checked inside the loop body
//! (not in the select) to ensure it fires on the exact boundary.
//!
//! # Design (D-08)
//!
//! - `tokio::select!` with two branches:
//!   - `agent_loop()` — the loop future, carrying its own iteration counter
//!   - `tokio::time::sleep` — the wall-clock timeout, fires unconditionally
//! - Iteration limit checked inside the loop body (exact boundary)
//! - Wall-clock timeout cannot be reset or extended by any code path

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::time::Duration;

use jadepaw_core::{AgentTerminationReason, JadepawError, ReActStep};

use crate::r#loop::LoopErrorKind;

/// Configuration for termination guards.
#[derive(Clone, Serialize, Deserialize)]
pub struct GuardConfig {
    /// Maximum number of ReAct loop iterations.
    pub max_iterations: u32,
    /// Maximum wall-clock time allowed for the entire agent run.
    pub wall_clock_timeout: Duration,
    /// Number of recent turns to preserve verbatim during context compression.
    /// Default: 5 (D-01).
    pub recent_turns: u32,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            wall_clock_timeout: Duration::from_secs(300),
            recent_turns: 5,
        }
    }
}

impl GuardConfig {
    /// Return the configured number of recent turns to preserve verbatim
    /// during context window compression.
    pub fn recent_turns(&self) -> u32 {
        self.recent_turns
    }
}

/// Run the agent loop with termination protection.
///
/// Races the agent loop future against a wall-clock timeout via `tokio::select!`.
/// The loop future carries its own iteration limit check internally.
///
/// # Type Parameters
///
/// - `F`: A closure that returns the agent loop future
/// - `Fut`: The agent loop future, which returns the execution trace
pub async fn run_with_guard<F, Fut>(
    config: &GuardConfig,
    agent_loop: F,
) -> Result<Vec<ReActStep>, JadepawError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<ReActStep>>>,
{
    // Record start time to compute actual elapsed wall-clock time for
    // timeout termination (WR-03). The configured limit is passed as max_ms.
    let start = tokio::time::Instant::now();

    tokio::select! {
        result = agent_loop() => {
            result.map_err(|e| {
                // Classify the error using structured downcast instead of
                // fragile string matching. The loop uses LoopErrorKind
                // variants that carry typed context for correct
                // AgentTerminationReason construction.
                if let Some(kind) = e.downcast_ref::<LoopErrorKind>() {
                    match kind {
                        LoopErrorKind::MaxIterations { iter, max } => {
                            JadepawError::agent_terminated(
                                AgentTerminationReason::MaxIterationsReached {
                                    iter: *iter,
                                    max: *max,
                                },
                            )
                        }
                        LoopErrorKind::LlmFailure { turn, source: _ } => {
                            JadepawError::agent_terminated(
                                AgentTerminationReason::InfrastructureError {
                                    reason: e.to_string(),
                                    turn: *turn,
                                },
                            )
                        }
                        LoopErrorKind::StoreFailure { turn, source: _ } => {
                            JadepawError::agent_terminated(
                                AgentTerminationReason::InfrastructureError {
                                    reason: e.to_string(),
                                    turn: *turn,
                                },
                            )
                        }
                        LoopErrorKind::ChannelClosed { turn } => {
                            JadepawError::agent_terminated(
                                AgentTerminationReason::InfrastructureError {
                                    reason: format!("client disconnected: {}", e),
                                    turn: *turn,
                                },
                            )
                        }
                    }
                } else {
                    // Fallback: unknown errors are classified as infrastructure
                    // errors (not Wasm traps). We may still be able to extract
                    // a turn number from the error message if the loop annotated
                    // it with the |turn=N| marker.
                    let err_msg = e.to_string();
                    match extract_turn_from_error(&err_msg) {
                        Some(turn) => {
                            JadepawError::agent_terminated(
                                AgentTerminationReason::InfrastructureError {
                                    reason: err_msg,
                                    turn,
                                },
                            )
                        }
                        None => {
                            // Turn could not be parsed — the error happened
                            // outside the loop or the marker was not present.
                            // Use a sentinel turn value to indicate "unknown"
                            // rather than collapsing to 0.
                            JadepawError::agent_terminated(
                                AgentTerminationReason::InfrastructureError {
                                    reason: err_msg,
                                    turn: u32::MAX,
                                },
                            )
                        }
                    }
                }
            })
        }

        _ = tokio::time::sleep(config.wall_clock_timeout) => {
            let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            let max_ms = u64::try_from(config.wall_clock_timeout.as_millis()).unwrap_or(u64::MAX);
            Err(JadepawError::agent_terminated(
                AgentTerminationReason::WallClockTimeout {
                    elapsed_ms,
                    max_ms,
                },
            ))
        }
    }
}

/// Attempt to extract a turn number from an error message.
///
/// The loop uses a structured marker `|turn=N|` in anyhow context messages
/// so that parsing does not rely on substring matching of natural-language
/// text that could appear in arbitrary source error messages.
///
/// Returns `None` if the turn cannot be determined, so callers can
/// distinguish between "error on turn 0" and "turn could not be parsed"
/// — unlike the previous `u32` return type where both cases produced 0.
fn extract_turn_from_error(err_msg: &str) -> Option<u32> {
    if let Some(turn_pos) = err_msg.find("|turn=") {
        let after = &err_msg[turn_pos + "|turn=".len()..];
        let turn_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(turn) = turn_str.parse::<u32>() {
            return Some(turn);
        }
    }
    None
}