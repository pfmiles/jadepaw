//! Tests for the ReAct loop orchestrator: structured trace output,
//! LLM integration (mocked), and run_agent() composition.
//!
//! These tests use a minimal InstancePool with a no-op WAT module and
//! a mock LlmProvider that returns fixed responses.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use jadepaw_agent::{react_loop, LlmProvider, LoopConfig, run_agent, run_with_guard, GuardConfig};
use jadepaw_core::{AgentRequest, AgentTerminationReason, ReActStep};
use jadepaw_wasm::{InstancePool, PoolConfig};
use tokio::sync::mpsc;

/// Helper: compile a minimal no-op WAT module to Wasm bytes.
fn noop_wasm_bytes() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "_start")))"#,
    )
    .expect("failed to parse noop.wat")
}

/// Helper: create a minimal InstancePool for loop tests.
fn make_test_pool() -> InstancePool {
    let config = PoolConfig::new(
        noop_wasm_bytes(),
        PathBuf::from("/tmp/jadepaw-test"),
        10,
    );
    InstancePool::new(config).expect("failed to create test pool")
}

/// A mock LLM provider that returns a fixed response.
struct MockLlm {
    response: String,
}

#[async_trait]
impl LlmProvider for MockLlm {
    async fn chat(&self, _messages: &[String]) -> anyhow::Result<String> {
        Ok(self.response.clone())
    }
}

/// A mock LLM provider that fails on every call.
struct FailingLlm;

#[async_trait]
impl LlmProvider for FailingLlm {
    async fn chat(&self, _messages: &[String]) -> anyhow::Result<String> {
        anyhow::bail!("mock LLM failure")
    }
}

// ── Test: react_loop produces thought and finished ─────────────────

#[tokio::test(flavor = "multi_thread")]
async fn react_loop_produces_thought_and_finished() {
    let pool = make_test_pool();
    let session_id = jadepaw_core::SessionId::new();
    let state = jadepaw_wasm::SessionState::with_defaults(PathBuf::from("/tmp/jadepaw-test"));
    let mut handle = pool
        .acquire(session_id, state)
        .await
        .expect("acquire failed");

    let config = LoopConfig::default();
    let mock = MockLlm {
        response: "I think the answer is 42".to_string(),
    };
    let (tx, mut rx) = mpsc::channel::<ReActStep>(256);

    let trace = react_loop(&config, &mut handle, &mock, &tx)
        .await
        .expect("react_loop failed");

    // Verify trace has Thought and Finished
    assert!(
        trace.len() >= 2,
        "expected at least 2 steps (Thought + Finished), got {}",
        trace.len()
    );
    assert!(
        matches!(&trace[0], ReActStep::Thought { .. }),
        "first step should be Thought, got: {:?}",
        trace[0]
    );
    assert!(
        matches!(&trace.last().unwrap(), ReActStep::Finished { .. }),
        "last step should be Finished, got: {:?}",
        trace.last().unwrap()
    );

    // Verify channel received both steps
    drop(tx);
    let channel_steps: Vec<ReActStep> = {
        let mut v = Vec::new();
        while let Ok(step) = rx.try_recv() {
            v.push(step);
        }
        v
    };
    assert_eq!(channel_steps.len(), 2, "channel should have 2 steps");
    assert!(
        matches!(&channel_steps[0], ReActStep::Thought { .. }),
        "channel[0] should be Thought"
    );
    assert!(
        matches!(&channel_steps[1], ReActStep::Finished { .. }),
        "channel[1] should be Finished"
    );
}

// ── Test: LLM failure produces error ───────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn react_loop_llm_failure_returns_error() {
    let pool = make_test_pool();
    let session_id = jadepaw_core::SessionId::new();
    let state = jadepaw_wasm::SessionState::with_defaults(PathBuf::from("/tmp/jadepaw-test"));
    let mut handle = pool
        .acquire(session_id, state)
        .await
        .expect("acquire failed");

    let config = LoopConfig::default();
    let failing = FailingLlm;
    let (tx, _rx) = mpsc::channel::<ReActStep>(256);

    let result = react_loop(&config, &mut handle, &failing, &tx).await;
    assert!(
        result.is_err(),
        "expected error from failing LLM, got: {result:?}"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("LLM call failed"),
        "expected 'LLM call failed' in error: {err_msg}"
    );
}

// ── Test: run_agent returns structured response ────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_agent_returns_structured_response() {
    let pool = Arc::new(make_test_pool());
    let session_id = jadepaw_core::SessionId::new();
    let req = AgentRequest {
        session_id,
        user_message: "what is the meaning of life?".to_string(),
        context: None,
    };
    let mock = MockLlm {
        response: "The answer is 42".to_string(),
    };

    let resp = run_agent(req, pool, &mock)
        .await
        .expect("run_agent failed");

    // Verify response structure
    assert_eq!(resp.session_id, session_id);
    assert!(!resp.final_answer.is_empty(), "final_answer should not be empty");
    assert!(
        !resp.trace.is_empty(),
        "trace should have at least one step"
    );

    // Verify the final answer comes from the Finished step
    let has_finished = resp
        .trace
        .iter()
        .any(|step| matches!(step, ReActStep::Finished { .. }));
    assert!(has_finished, "trace should contain a Finished step");

    // Verify the trace matches the final answer
    if let Some(last_finished) = resp.trace.iter().rev().find_map(|step| match step {
        ReActStep::Finished { answer } => Some(answer.clone()),
        _ => None,
    }) {
        assert_eq!(last_finished, resp.final_answer);
    }
}

// ── Test: run_agent propagation through guard ──────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_agent_with_fast_timeout_triggers_guard() {
    let pool = Arc::new(make_test_pool());
    let session_id = jadepaw_core::SessionId::new();
    let _req = AgentRequest {
        session_id,
        user_message: "hello".to_string(),
        context: None,
    };
    // This mock sleeps long enough to trigger a timeout
    struct SlowMock;
    #[async_trait]
    impl LlmProvider for SlowMock {
        async fn chat(&self, _messages: &[String]) -> anyhow::Result<String> {
            tokio::time::sleep(std::time::Duration::from_secs(999)).await;
            Ok("too late".to_string())
        }
    }

    // Use a short timeout config through the composition
    // We test this via run_with_guard directly since run_agent uses defaults
    let state = jadepaw_wasm::SessionState::with_defaults(PathBuf::from("/tmp/jadepaw-test"));
    let mut handle = pool
        .acquire(session_id, state)
        .await
        .expect("acquire failed");

    let loop_config = LoopConfig::default();
    let mock = SlowMock;
    let (tx, _rx) = mpsc::channel::<ReActStep>(256);
    let guard_config = GuardConfig {
        max_iterations: 20,
        wall_clock_timeout: std::time::Duration::from_millis(50),
    };

    let result = run_with_guard(guard_config, || {
        react_loop(&loop_config, &mut handle, &mock, &tx)
    })
    .await;

    assert!(result.is_err(), "expected timeout error, got: {result:?}");
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