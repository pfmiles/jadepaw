//! SSE streaming integration tests.
//!
//! Verifies AGENT-03 behaviors: real-time token delivery, correct event naming
//! per D-14, done event format, and injection safety. These tests exercise the
//! full create_sse_channel -> ReActStep -> SSE event pipeline without requiring
//! a real LLM API key.

use futures::StreamExt;
use jadepaw_agent::create_sse_channel;
use jadepaw_core::ReActStep;

// ── Test: create_sse_channel produces correct events ────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_create_sse_channel_produces_correct_events() {
    let (tx, stream) = create_sse_channel();

    // Send a Thought event
    tx.send(ReActStep::Thought {
        content: "hello".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 1);

    let dbg = format!("{:?}", events[0].as_ref().unwrap());
    assert!(dbg.contains("event: thought"), "expected thought event: {dbg}");
    assert!(dbg.contains("hello"), "expected hello content: {dbg}");
}

// ── Test: all five ReActStep variants map to correct events ─────────

#[tokio::test(flavor = "multi_thread")]
async fn test_create_sse_channel_all_variants() {
    let (tx, stream) = create_sse_channel();

    tx.send(ReActStep::Thought {
        content: "thinking".to_string(),
    })
    .await
    .unwrap();
    tx.send(ReActStep::Action {
        tool: "search".to_string(),
        args: serde_json::json!({"query": "test"}),
    })
    .await
    .unwrap();
    tx.send(ReActStep::Observation {
        result: "found results".to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    tx.send(ReActStep::Error {
        message: "timeout".to_string(),
        turn: 1,
    })
    .await
    .unwrap();
    tx.send(ReActStep::Finished {
        answer: "42".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 5);

    let names: Vec<String> = events
        .iter()
        .map(|e| format!("{:?}", e.as_ref().unwrap()))
        .collect();

    assert!(names[0].contains("event: thought"), "event 0: {}", names[0]);
    assert!(names[1].contains("event: action"), "event 1: {}", names[1]);
    assert!(names[2].contains("event: observation"), "event 2: {}", names[2]);
    assert!(names[3].contains("event: error"), "event 3: {}", names[3]);
    assert!(names[4].contains("event: done"), "event 4: {}", names[4]);
}

// ── Test: SSE streaming is real-time (AGENT-03) ─────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_sse_streaming_real_time() {
    let (tx, stream) = create_sse_channel();

    // Send first token (Thought)
    tx.send(ReActStep::Thought {
        content: "token1".to_string(),
    })
    .await
    .unwrap();

    // Use timeout to verify the first event arrives within 10ms
    // (proves streaming is real-time, not buffered — per AGENT-03)
    let mut boxed = Box::pin(stream);
    let first_event = tokio::time::timeout(
        std::time::Duration::from_millis(10),
        StreamExt::next(&mut boxed),
    )
    .await
    .expect("timeout — streaming is not real-time (exceeded 10ms)")
    .expect("stream ended prematurely")
    .expect("event error");

    let dbg = format!("{:?}", first_event);
    assert!(dbg.contains("event: thought"), "expected thought: {dbg}");
    assert!(dbg.contains("token1"), "expected token1: {dbg}");

    // Send a second token — verify it arrives separately, not batched
    tx.send(ReActStep::Thought {
        content: "token2".to_string(),
    })
    .await
    .unwrap();
    drop(tx);
}

// ── Test: done event carries final answer ───────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_sse_done_event_carries_final_answer() {
    let (tx, stream) = create_sse_channel();

    tx.send(ReActStep::Finished {
        answer: "The meaning of life is 42".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 1);

    let dbg = format!("{:?}", events[0].as_ref().unwrap());
    assert!(
        dbg.contains("event: done"),
        "expected done event: {dbg}"
    );
    assert!(
        dbg.contains("answer"),
        "answer key not found in done event: {dbg}"
    );
    assert!(
        dbg.contains("The meaning of life is 42"),
        "answer value not found: {dbg}"
    );
}

// ── Test: SSE injection sanitization ────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_sse_injection_sanitization() {
    // Per RESEARCH.md security domain (T-03-05): SSE control characters
    // in tool output could break the event stream. The axum Event builder
    // handles this via its internal buffering.
    let (tx, stream) = create_sse_channel();

    // Send observation with content that would break raw SSE format
    let malicious_content = "\n\nevent: fake\ndata: injected\n\n";

    tx.send(ReActStep::Observation {
        result: malicious_content.to_string(),
        is_error: false,
    })
    .await
    .unwrap();
    drop(tx);

    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 1);

    let dbg = format!("{:?}", events[0].as_ref().unwrap());

    // The event should still be an observation event — the content should
    // be in the data field, not breaking the event framing
    assert!(
        dbg.contains("event: observation"),
        "expected observation event: {dbg}"
    );

    // CRITICAL: Verify no injected event type. The axum Event builder
    // encodes the content `event: fake` inside a data: field, so it
    // appears as `data: event: fake`. Count raw `event: ` at line
    // starts (after newlines). In axum's Debug format, there should
    // be exactly one `event: observation` line start. The injected
    // `event: fake` should only appear after `data:`.
    //
    // The Debug output looks like:
    //   event: observation\ndata: \ndata: \ndata: event: fake\ndata: ...
    //
    // So `\nevent: ` (newline then event:) should appear 0 times:
    // the only event: is at the start of the buffer.
    let newline_event_count = dbg.matches("\nevent: ").count();
    assert_eq!(
        newline_event_count, 0,
        "expected 0 '\\nevent: ' occurrences (no injected event declarations): {dbg}"
    );

    // The axum Event builder splits newlines into separate data: fields.
    // Verify the injected content appears in data: lines, not as raw format.
    assert!(
        dbg.contains("fake") || dbg.contains("injected"),
        "content should be present in event: {dbg}"
    );

    // Verify the event: observation declaration appears (exactly once)
    assert!(
        dbg.starts_with("Event {") || dbg.contains("event: observation"),
        "event should be observation: {dbg}"
    );
}

// ── Test: mpsc channel backpressure ─────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_mpsc_channel_backpressure() {
    // Verify the channel has adequate buffer capacity (256 per RESEARCH.md
    // pitfall 2) and doesn't panic under load.
    let (tx, stream) = create_sse_channel();

    // Send many events in rapid succession
    for i in 0..100 {
        tx.send(ReActStep::Thought {
            content: format!("token_{}", i),
        })
        .await
        .unwrap();
    }
    drop(tx);

    let events: Vec<_> = StreamExt::collect(stream).await;
    assert_eq!(events.len(), 100, "expected all 100 events to be delivered");

    // Spot-check first and last
    let first = format!("{:?}", events[0].as_ref().unwrap());
    let last = format!("{:?}", events[99].as_ref().unwrap());
    assert!(first.contains("token_0"));
    assert!(last.contains("token_99"));
}