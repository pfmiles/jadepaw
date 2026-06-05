//! Tests for agent types: AgentRequest, AgentResponse, ReActStep,
//! and AgentTerminationReason.
//!
//! Verifies serde roundtrips for all types, default values, and
//! Display implementations for termination reasons.

use jadepaw_core::{AgentRequest, AgentResponse, AgentTerminationReason, ReActStep};

// ── Test: AgentRequest default ─────────────────────────────────────

#[test]
fn agent_request_default_has_non_empty_session_id() {
    let req = AgentRequest::default();
    let s = format!("{}", req.session_id);
    assert!(!s.is_empty(), "session_id should produce a non-empty display");
}

#[test]
fn agent_request_serde_roundtrip() {
    let req = AgentRequest::default();
    let json = serde_json::to_string(&req).expect("serialize failed");
    let roundtripped: AgentRequest = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(req, roundtripped);
}

#[test]
fn agent_request_with_context() {
    let req = AgentRequest {
        session_id: jadepaw_core::SessionId::new(),
        user_message: "hello".to_string(),
        context: Some("be helpful".to_string()),
        resume_from: None,
    };
    assert_eq!(req.user_message, "hello");
    assert_eq!(req.context, Some("be helpful".to_string()));
}

// ── Test: AgentResponse ────────────────────────────────────────────

#[test]
fn agent_response_serde_roundtrip() {
    let session_id = jadepaw_core::SessionId::new();
    let resp = AgentResponse {
        session_id,
        final_answer: "the answer is 42".to_string(),
        trace: vec![
            ReActStep::Thought {
                content: "let me think...".to_string(),
            },
            ReActStep::Finished {
                answer: "the answer is 42".to_string(),
            },
        ],
    };
    let json = serde_json::to_string(&resp).expect("serialize failed");
    let roundtripped: AgentResponse = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(resp, roundtripped);
    assert_eq!(roundtripped.final_answer, "the answer is 42");
    assert_eq!(roundtripped.trace.len(), 2);
}

// ── Test: ReActStep variants ───────────────────────────────────────

#[test]
fn react_step_thought_serde_roundtrip() {
    let step = ReActStep::Thought {
        content: "I need to look this up".to_string(),
    };
    let json = serde_json::to_string(&step).expect("serialize failed");
    let roundtripped: ReActStep = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(step, roundtripped);
}

#[test]
fn react_step_action_serde_roundtrip() {
    let step = ReActStep::Action {
        tool: "file_read".to_string(),
        args: serde_json::json!({"path": "/tmp/test.txt"}),
    };
    let json = serde_json::to_string(&step).expect("serialize failed");
    let roundtripped: ReActStep = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(step, roundtripped);
}

#[test]
fn react_step_observation_serde_roundtrip() {
    let step = ReActStep::Observation {
        result: "file contents: hello world".to_string(),
        is_error: false,
    };
    let json = serde_json::to_string(&step).expect("serialize failed");
    let roundtripped: ReActStep = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(step, roundtripped);
}

#[test]
fn react_step_error_serde_roundtrip() {
    let step = ReActStep::Error {
        message: "something went wrong".to_string(),
        turn: 3,
    };
    let json = serde_json::to_string(&step).expect("serialize failed");
    let roundtripped: ReActStep = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(step, roundtripped);
}

#[test]
fn react_step_finished_serde_roundtrip() {
    let step = ReActStep::Finished {
        answer: "the answer is 42".to_string(),
    };
    let json = serde_json::to_string(&step).expect("serialize failed");
    let roundtripped: ReActStep = serde_json::from_str(&json).expect("deserialize failed");
    assert_eq!(step, roundtripped);
}

#[test]
fn react_step_variants_are_distinct() {
    let thought = ReActStep::Thought {
        content: "x".to_string(),
    };
    let finished = ReActStep::Finished {
        answer: "y".to_string(),
    };
    assert_ne!(thought, finished);
}

// ── Test: AgentTerminationReason Display ───────────────────────────

#[test]
fn termination_reason_display_max_iterations() {
    let reason = AgentTerminationReason::MaxIterationsReached { iter: 20, max: 20 };
    let s = reason.to_string();
    assert!(s.contains("max iterations"), "expected 'max iterations' in: {s}");
    assert!(s.contains("20"), "expected '20' in: {s}");
}

#[test]
fn termination_reason_display_wall_clock_timeout() {
    let reason = AgentTerminationReason::WallClockTimeout {
        elapsed_ms: 300_000,
        max_ms: 300_000,
    };
    let s = reason.to_string();
    assert!(s.contains("timed out"), "expected 'timed out' in: {s}");
    assert!(s.contains("300"), "expected '300' in: {s}");
}

#[test]
fn termination_reason_display_wasm_trap() {
    let reason = AgentTerminationReason::WasmTrap {
        reason: "out of bounds memory access".to_string(),
        turn: 5,
    };
    let s = reason.to_string();
    assert!(s.contains("wasm"), "expected 'wasm' in: {s}");
    assert!(s.contains("out of bounds"), "expected trap reason in: {s}");
    assert!(s.contains("5"), "expected turn 5 in: {s}");
}

// ── Test: JadepawError agent_terminated variant ────────────────────

#[test]
fn jadepaw_error_agent_terminated_display() {
    let reason = AgentTerminationReason::MaxIterationsReached { iter: 5, max: 5 };
    let err = jadepaw_core::JadepawError::agent_terminated(reason);
    let s = err.to_string();
    assert!(s.contains("agent terminated"), "expected 'agent terminated' in: {s}");
    assert!(s.contains("max iterations"), "expected 'max iterations' in: {s}");
}