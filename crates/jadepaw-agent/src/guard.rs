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
    config: GuardConfig,
    agent_loop: F,
) -> Result<Vec<ReActStep>, JadepawError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<ReActStep>>>,
{
    tokio::select! {
        result = agent_loop() => {
            result.map_err(|e| {
                let err_msg = e.to_string();

                // Classify the error to select the correct termination reason.
                // The agent loop uses structured anyhow error messages that
                // encode the failure mode and turn number.

                // Extract turn number from error message if present
                let turn = extract_turn_from_error(&err_msg);

                if err_msg.contains("max iterations") {
                    JadepawError::agent_terminated(
                        AgentTerminationReason::MaxIterationsReached {
                            iter: turn,
                            max: config.max_iterations,
                        },
                    )
                } else if err_msg.contains("LLM call failed") {
                    // LLM failures are infrastructure errors, not Wasm traps.
                    // Callers can distinguish transient network errors from
                    // actual guest sandbox violations for retry/monitoring.
                    JadepawError::agent_terminated(
                        AgentTerminationReason::InfrastructureError {
                            reason: format!("LLM error: {}", err_msg),
                            turn,
                        },
                    )
                } else if err_msg.contains("output channel closed") {
                    // Channel closure is a client disconnect, not a trap.
                    // Map to InfrastructureError so callers know the agent
                    // didn't crash — the client went away.
                    JadepawError::agent_terminated(
                        AgentTerminationReason::InfrastructureError {
                            reason: format!("client disconnected: {}", err_msg),
                            turn,
                        },
                    )
                } else {
                    // Fallback: unknown errors are classified as traps with the
                    // original error message preserved for debugging
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
            Err(JadepawError::agent_terminated(
                AgentTerminationReason::WallClockTimeout {
                    elapsed_secs: config.wall_clock_timeout.as_secs(),
                    max_secs: config.wall_clock_timeout.as_secs(),
                },
            ))
        }
    }
}

/// Attempt to extract a turn number from a loop error message.
///
/// The loop uses `anyhow` error messages containing "on turn N". This
/// function parses that pattern and returns the turn number, defaulting
/// to 0 if the turn cannot be determined.
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