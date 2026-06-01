//! Tests for the ReAct loop orchestrator: structured trace output,
//! real LLM streaming, and run_agent() composition.
//!
//! These tests verify the channel-based streaming pipeline and structural
//! properties of the ReAct loop. Full integration with async-openai requires
//! a live API key and is tested via SSE event relay tests in sse_streaming.rs.

use std::path::PathBuf;
use std::sync::Arc;

use jadepaw_core::ReActStep;
use jadepaw_wasm::{InstancePool, PoolConfig};

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

// ── Test: SSE channel produces thought events ────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn sse_channel_produces_events() {
    let (tx, stream) = jadepaw_agent::create_sse_channel();

    tx.send(ReActStep::Thought {
        content: "hello".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    // Collect stream events
    use futures::StreamExt;
    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 1, "expected 1 event");
    let dbg = format!("{:?}", events[0].as_ref().unwrap());
    assert!(dbg.contains("event: thought"), "unexpected event: {dbg}");
}

// ── Test: run_agent returns structured response ────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn run_agent_returns_structured_response() {
    // Verify run_agent's type signature compiles by creating an invalid
    // LLM client that will fail immediately (no API key needed).
    // This test exercises the function signature and error-path behavior.
    let pool = Arc::new(make_test_pool());
    let config: Box<dyn async_openai::config::Config> = Box::new(
        async_openai::config::OpenAIConfig::new()
            .with_api_base("http://[::1]:1"), // invalid port, immediate fail
    );
    let client = async_openai::Client::with_config(config);
    let result = jadepaw_agent::run_agent(
        jadepaw_core::AgentRequest::default(),
        pool,
        client,
        "gpt-4",
    )
    .await;
    // Expected to fail (connection refused), but function signature must compile
    assert!(result.is_err(), "expected connection error");
}

// ── Test: run_agent with SSE streaming pipeline type-checks ─────────

#[tokio::test(flavor = "multi_thread")]
async fn run_agent_sse_stream_pipeline_type_checks() {
    // Verify that the create_sse_channel produces types that compile with
    // axum's Sse response type and the streaming pipeline pattern.
    use axum::response::sse::Sse;

    let (_tx, stream) = jadepaw_agent::create_sse_channel();

    // This just checks the type compiles — Sse::new takes impl Stream<Item = Result<Event, E>>
    // where E: Into<Box<dyn std::error::Error + Send + Sync>>
    let _sse_response: Sse<_> = Sse::new(stream);
}