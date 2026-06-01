//! SSE token relay.
//!
//! Converts `ReActStep` events to named SSE events via an mpsc channel.
//! Maps to D-14 event naming: `thought`, `action`, `observation`, `error`,
//! `done`. The `create_sse_channel()` function returns a sender/receiver pair
//! that connects the LLM streaming pipeline to an axum SSE response.
//!
//! # Design (D-07, D-14)
//!
//! - Streaming pipeline: `ChatCompletionStream` -> `mpsc::channel(256)` ->
//!   `ReceiverStream` -> `axum::response::Sse`
//! - Each ReActStep variant maps to a named SSE event
//! - Per RESEARCH.md pitfall 4: always use axum `Event` builder — never
//!   manually format SSE strings
//! - Per RESEARCH.md pitfall 2: 256 buffer provides backpressure without
//!   excessive memory

use axum::response::sse::Event;
use futures::stream::Stream;
use jadepaw_core::ReActStep;
use std::convert::Infallible;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

/// Create an mpsc channel for ReActStep events, returning the sender and an
/// SSE-compatible stream suitable for axum's `Sse` response type.
///
/// # Returns
///
/// A tuple of `(mpsc::Sender<ReActStep>, impl Stream<Item = Result<Event, Infallible>>)`:
/// - The sender is used by the agent loop (or LLM streaming code) to push
///   ReActStep events in real time.
/// - The stream can be passed directly to `axum::response::Sse::new()`.
///
/// # Channel capacity
///
/// The channel has a buffer of 256 items per RESEARCH.md pitfall 2. This
/// provides backpressure without excessive memory usage.
///
/// # Event naming (D-14)
///
/// | ReActStep variant | SSE event name | Data format |
/// |-------------------|----------------|-------------|
/// | `Thought`         | `thought`      | Plain text  |
/// | `Action`          | `action`       | JSON `{"tool": "...", "args": "..."}` |
/// | `Observation`     | `observation`  | Plain text  |
/// | `Error`           | `error`        | JSON `{"message": "...", "turn": N}` |
/// | `Finished`        | `done`         | JSON `{"answer": "..."}` |
pub fn create_sse_channel(
) -> (mpsc::Sender<ReActStep>, impl Stream<Item = Result<Event, Infallible>>) {
    let (tx, rx) = mpsc::channel::<ReActStep>(256);

    let stream = ReceiverStream::new(rx).map(|step| {
        let event = match step {
            ReActStep::Thought { content } => {
                Event::default().event("thought").data(content)
            }
            ReActStep::Action { tool, args } => {
                let payload = serde_json::to_string(&serde_json::json!({
                    "tool": tool,
                    "args": args,
                }))
                .unwrap_or_default();
                Event::default().event("action").data(payload)
            }
            ReActStep::Observation { result } => {
                Event::default().event("observation").data(result)
            }
            ReActStep::Error { message, turn } => {
                let payload = serde_json::to_string(&serde_json::json!({
                    "message": message,
                    "turn": turn,
                }))
                .unwrap_or_default();
                Event::default().event("error").data(payload)
            }
            ReActStep::Finished { answer } => {
                let payload = serde_json::to_string(&serde_json::json!({
                    "answer": answer,
                }))
                .unwrap_or_default();
                Event::default().event("done").data(payload)
            }
        };

        Ok(event)
    });

    (tx, stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt as FuturesStreamExt;

    #[tokio::test]
    async fn channel_produces_thought_event() {
        let (tx, stream) = create_sse_channel();

        tx.send(ReActStep::Thought {
            content: "hello".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        let events: Vec<Result<Event, Infallible>> =
            FuturesStreamExt::collect(stream).await;
        assert_eq!(events.len(), 1);
        let dbg = format!("{:?}", events[0].as_ref().unwrap());
        assert!(
            dbg.contains("event: thought"),
            "expected 'event: thought' in debug: {dbg}"
        );
        assert!(
            dbg.contains("hello"),
            "expected 'hello' in debug: {dbg}"
        );
    }

    #[tokio::test]
    async fn channel_produces_all_variants() {
        let (tx, stream) = create_sse_channel();

        tx.send(ReActStep::Thought {
            content: "thinking...".to_string(),
        })
        .await
        .unwrap();
        tx.send(ReActStep::Action {
            tool: "search".to_string(),
            args: serde_json::json!({"query": "rust"}),
        })
        .await
        .unwrap();
        tx.send(ReActStep::Observation {
            result: "found 10 results".to_string(),
        })
        .await
        .unwrap();
        tx.send(ReActStep::Error {
            message: "timeout".to_string(),
            turn: 2,
        })
        .await
        .unwrap();
        tx.send(ReActStep::Finished {
            answer: "42".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        let events: Vec<Result<Event, Infallible>> =
            FuturesStreamExt::collect(stream).await;

        assert_eq!(events.len(), 5);

        let names: Vec<String> = events
            .iter()
            .map(|e| format!("{:?}", e.as_ref().unwrap()))
            .collect();

        // Verify event names in order via Debug representation
        assert!(names[0].contains("event: thought"), "expected thought");
        assert!(names[1].contains("event: action"), "expected action");
        assert!(names[2].contains("event: observation"), "expected observation");
        assert!(names[3].contains("event: error"), "expected error");
        assert!(names[4].contains("event: done"), "expected done");

        // Verify action event data contains JSON with tool/args
        assert!(
            names[1].contains("tool"),
            "action event missing tool: {}",
            names[1]
        );
        assert!(
            names[1].contains("args"),
            "action event missing args: {}",
            names[1]
        );

        // Verify done event data contains JSON with answer
        assert!(
            names[4].contains("answer"),
            "done event: {}",
            names[4]
        );
        assert!(
            names[4].contains("42"),
            "done event missing answer value: {}",
            names[4]
        );
    }

    #[tokio::test]
    async fn streaming_is_real_time() {
        let (tx, stream) = create_sse_channel();

        // Send a Thought first
        tx.send(ReActStep::Thought {
            content: "first".to_string(),
        })
        .await
        .unwrap();

        // Use timeout to verify the first event arrives quickly (real-time, not buffered)
        let mut boxed = Box::pin(stream);
        let event = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            FuturesStreamExt::next(&mut boxed),
        )
        .await
        .expect("timeout — streaming is not real-time")
        .expect("stream ended prematurely")
        .expect("event error");

        let dbg = format!("{:?}", event);
        assert!(
            dbg.contains("event: thought"),
            "expected thought event: {dbg}"
        );
        assert!(
            dbg.contains("first"),
            "expected 'first' in event: {dbg}"
        );

        // Now send Finished and verify it arrives as second event
        tx.send(ReActStep::Finished {
            answer: "done".to_string(),
        })
        .await
        .unwrap();
        drop(tx);
    }

    #[tokio::test]
    async fn done_event_carries_final_answer() {
        let (tx, stream) = create_sse_channel();

        tx.send(ReActStep::Finished {
            answer: "the ultimate answer is 42".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        let events: Vec<Result<Event, Infallible>> =
            FuturesStreamExt::collect(stream).await;
        assert_eq!(events.len(), 1);

        let dbg = format!("{:?}", events[0].as_ref().unwrap());
        assert!(
            dbg.contains("event: done"),
            "expected 'event: done' in: {dbg}"
        );
        assert!(
            dbg.contains("the ultimate answer is 42"),
            "expected answer content in done event: {dbg}"
        );
    }

    #[tokio::test]
    async fn injection_sanitization_double_newline() {
        // Per RESEARCH.md security domain: SSE control characters in tool output
        // could break the event stream. The axum Event builder handles
        // newline encoding via its internal buffer.
        let (tx, stream) = create_sse_channel();

        // Send an Observation with content containing double-newline (would break
        // raw SSE format if not handled)
        tx.send(ReActStep::Observation {
            result: "line1\n\nline2".to_string(),
        })
        .await
        .unwrap();
        drop(tx);

        let events: Vec<Result<Event, Infallible>> =
            FuturesStreamExt::collect(stream).await;
        assert_eq!(events.len(), 1);

        let dbg = format!("{:?}", events[0].as_ref().unwrap());
        assert!(
            dbg.contains("event: observation"),
            "expected observation event: {dbg}"
        );

        // The axum Event builder splits newlines into separate data: fields,
        // so the Debug output should contain the content broken across lines.
        assert!(
            dbg.contains("line1") && dbg.contains("line2"),
            "observation data should contain original content: {dbg}"
        );
    }
}