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
                // Map anyhow errors from the loop to WasmTrap termination reasons.
                // In Plan 02, we'll add finer-grained error classification here.
                JadepawError::agent_terminated(
                    AgentTerminationReason::WasmTrap {
                        reason: e.to_string(),
                        turn: 0, // approximate — loop-internal errors don't expose exact turn
                    },
                )
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