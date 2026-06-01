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

use std::future::Future;
use std::time::Duration;

use jadepaw_core::{AgentTerminationReason, JadepawError, ReActStep};

use crate::r#loop::LoopErrorKind;

/// Configuration for termination guards.
#[derive(Clone)]
pub struct GuardConfig {
    /// Maximum number of ReAct loop iterations.
    pub max_iterations: u32,
    /// Maximum wall-clock time allowed for the entire agent run.
    pub wall_clock_timeout: Duration,
}

impl Default for GuardConfig {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            wall_clock_timeout: Duration::from_secs(300),
        }
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
                    // Fallback: unknown errors are classified as traps with the
                    // original error message preserved for debugging.
                    let err_msg = e.to_string();
                    let turn = extract_turn_from_error(&err_msg);
                    JadepawError::agent_terminated(
                        AgentTerminationReason::WasmTrap {
                            reason: err_msg,
                            turn,
                        },
                    )
                }
            })
        }

        _ = tokio::time::sleep(config.wall_clock_timeout) => {
            let ms = config.wall_clock_timeout.as_millis() as u64;
            Err(JadepawError::agent_terminated(
                AgentTerminationReason::WallClockTimeout {
                    elapsed_ms: ms,
                    max_ms: ms,
                },
            ))
        }
    }
}

/// Attempt to extract a turn number from a loop error message.
///
/// The loop uses `anyhow` error messages containing "on turn N". This
/// function parses that pattern and returns the turn number. Returns
/// 0 if the turn cannot be determined (callers should note that 0 is
/// ambiguous between "error on turn 0" and "turn could not be parsed").
fn extract_turn_from_error(err_msg: &str) -> u32 {
    // Look for "on turn <N>" pattern in the error message
    if let Some(turn_pos) = err_msg.find("on turn ") {
        let after = &err_msg[turn_pos + "on turn ".len()..];
        let turn_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(turn) = turn_str.parse::<u32>() {
            return turn;
        }
    }
    0
}