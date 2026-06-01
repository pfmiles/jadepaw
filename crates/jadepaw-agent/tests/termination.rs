//! Tests for termination guards: wall-clock timeout, normal completion,
//! and error message verification.
//!
//! These tests exercise the `run_with_guard` function independently of
//! the Wasm runtime — they pass closures directly.

use std::time::Duration;

use jadepaw_agent::{run_with_guard, GuardConfig};
use jadepaw_core::AgentTerminationReason;

// ── Test: guard config default values ──────────────────────────────

#[test]
fn guard_config_default_values() {
    let config = GuardConfig::default();
    assert_eq!(config.max_iterations, 20);
    assert_eq!(config.wall_clock_timeout, Duration::from_secs(300));
}

// ── Test: normal completion ────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_with_guard_loop_completes() {
    let config = GuardConfig::default();
    let result = run_with_guard(config, || async {
        Ok(vec![])
    })
    .await;
    assert!(result.is_ok(), "expected Ok, got: {result:?}");
    assert_eq!(result.unwrap(), vec![]);
}

// ── Test: wall-clock timeout ───────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_with_guard_wall_clock_timeout() {
    let config = GuardConfig {
        max_iterations: 20,
        wall_clock_timeout: Duration::from_millis(50),
    };
    let result = run_with_guard(config, || async {
        // Sleep longer than the timeout — should be interrupted
        tokio::time::sleep(Duration::from_secs(999)).await;
        Ok(vec![])
    })
    .await;
    assert!(
        result.is_err(),
        "expected Err(WallClockTimeout), got: {result:?}"
    );
    match result.unwrap_err() {
        jadepaw_core::JadepawError::AgentTerminated { reason } => {
            assert!(
                matches!(reason, AgentTerminationReason::WallClockTimeout { .. }),
                "expected WallClockTimeout, got: {reason:?}"
            );
        }
        other => panic!("expected AgentTerminated, got: {other:?}"),
    }
}

// ── Test: timeout value is propagated correctly ────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_with_guard_timeout_value_propagated() {
    let config = GuardConfig {
        max_iterations: 20,
        wall_clock_timeout: Duration::from_millis(10),
    };
    let result = run_with_guard(config, || async {
        tokio::time::sleep(Duration::from_secs(999)).await;
        Ok(vec![])
    })
    .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        jadepaw_core::JadepawError::AgentTerminated { reason } => {
            match reason {
                AgentTerminationReason::WallClockTimeout { max_ms, .. } => {
                    // max_ms preserves millisecond precision (10ms = 10ms)
                    assert_eq!(max_ms, 10);
                }
                other => panic!("expected WallClockTimeout, got: {other:?}"),
            }
        }
        other => panic!("expected AgentTerminated, got: {other:?}"),
    }
}

// ── Test: loop error maps to WasmTrap ──────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_with_guard_maps_loop_error_to_wasm_trap() {
    let config = GuardConfig::default();
    let result = run_with_guard(config, || async {
        anyhow::bail!("test error from loop")
    })
    .await;
    assert!(result.is_err());
    match result.unwrap_err() {
        jadepaw_core::JadepawError::AgentTerminated { reason } => {
            assert!(
                matches!(reason, AgentTerminationReason::WasmTrap { .. }),
                "expected WasmTrap, got: {reason:?}"
            );
        }
        other => panic!("expected AgentTerminated, got: {other:?}"),
    }
}