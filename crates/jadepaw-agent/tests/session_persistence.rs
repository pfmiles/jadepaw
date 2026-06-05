//! Tests for session persistence: turn-boundary checkpoints, pause/resume
//! roundtrip, isolation, and crash recovery.
//!
//! These tests verify MEM-02 requirements: session state persists to SQLite
//! and can be recovered across restarts.

use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage,
};
use jadepaw_agent::GuardConfig;
use jadepaw_core::{ReActStep, SessionId, TenantId};
use jadepaw_db::{SessionRepository, SessionSnapshot, SessionStatus, SqliteSessionRepo};

/// Helper: create a user message.
fn user_msg(text: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestUserMessage::from(text).into()
}

/// Helper: create an assistant message.
fn assistant_msg(text: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestAssistantMessage::from(text).into()
}

/// Helper: create a system message.
fn system_msg(text: &str) -> ChatCompletionRequestMessage {
    ChatCompletionRequestSystemMessage::from(text).into()
}

/// Helper: create an in-memory SQLite repository.
async fn make_repo() -> SqliteSessionRepo {
    SqliteSessionRepo::new("sqlite://:memory:")
        .await
        .expect("failed to create in-memory SQLite repo")
}

/// Helper: build a basic SessionSnapshot for test pause/resume.
fn make_snapshot(
    session_id: SessionId,
    tenant_id: TenantId,
    messages: &[ChatCompletionRequestMessage],
    trace: &[ReActStep],
    guard_config: &GuardConfig,
) -> SessionSnapshot {
    SessionSnapshot {
        session_id,
        tenant_id,
        status: SessionStatus::Paused,
        messages_json: serde_json::to_string(messages).unwrap(),
        trace_json: serde_json::to_string(trace).unwrap(),
        guard_config_json: serde_json::to_string(guard_config).unwrap(),
        elapsed_ms: 0,
        iteration_count: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        termination_reason_json: None,
    }
}

// ── Test: save and load roundtrip ────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn save_and_load_roundtrip() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();

    let messages = vec![
        system_msg("You are a helpful assistant."),
        user_msg("Hello!"),
        assistant_msg("Hi there! How can I help?"),
    ];

    let trace = vec![ReActStep::Thought {
        content: "The user said hello.".to_string(),
    }];

    let guard_config = GuardConfig::default();
    let snapshot = make_snapshot(
        session_id,
        tenant_id,
        &messages,
        &trace,
        &guard_config,
    );

    // Save
    repo.save(session_id, tenant_id, snapshot.clone())
        .await
        .expect("failed to save snapshot");

    // Load
    let loaded = repo
        .load(session_id, tenant_id)
        .await
        .expect("failed to load snapshot")
        .expect("snapshot not found");

    assert_eq!(loaded.session_id, session_id);
    assert_eq!(loaded.tenant_id, tenant_id);
    assert_eq!(loaded.status, SessionStatus::Paused);

    // Verify JSON roundtrip
    let loaded_messages: Vec<ChatCompletionRequestMessage> =
        serde_json::from_str(&loaded.messages_json).unwrap();
    assert_eq!(loaded_messages.len(), 3);
}

// ── Test: delete is idempotent ──────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn delete_idempotent() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();

    // Delete on non-existent session should not error
    repo.delete(session_id, tenant_id)
        .await
        .expect("delete should be idempotent");

    // Save then delete
    let messages = vec![user_msg("test")];
    let trace: Vec<ReActStep> = vec![];
    let snapshot = make_snapshot(
        session_id,
        tenant_id,
        &messages,
        &trace,
        &GuardConfig::default(),
    );
    repo.save(session_id, tenant_id, snapshot)
        .await
        .expect("failed to save");

    repo.delete(session_id, tenant_id)
        .await
        .expect("delete should succeed");

    // Load should return None
    let loaded = repo.load(session_id, tenant_id).await.unwrap();
    assert!(loaded.is_none(), "session should be deleted");
}

// ── Test: crash recovery marks running as paused ─────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn crash_recovery_marks_running_as_paused() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();

    let messages = vec![user_msg("test")];
    let trace: Vec<ReActStep> = vec![];
    let snapshot = SessionSnapshot {
        session_id,
        tenant_id,
        status: SessionStatus::Running,
        messages_json: serde_json::to_string(&messages).unwrap(),
        trace_json: serde_json::to_string(&trace).unwrap(),
        guard_config_json: serde_json::to_string(&GuardConfig::default()).unwrap(),
        elapsed_ms: 0,
        iteration_count: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        termination_reason_json: None,
    };

    repo.save(session_id, tenant_id, snapshot)
        .await
        .expect("failed to save");

    // Crash recovery: mark all running as paused
    let affected = repo
        .mark_running_as_paused()
        .await
        .expect("mark_running_as_paused failed");

    assert_eq!(affected.len(), 1, "one session should be affected");
    assert_eq!(affected[0], session_id);

    // Verify status is now paused
    let loaded = repo.load(session_id, tenant_id).await.unwrap().unwrap();
    assert_eq!(loaded.status, SessionStatus::Paused);
}

// ── Test: session isolation (cross-tenant) ───────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn session_isolation_cross_tenant() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_a = TenantId::new();
    let tenant_b = TenantId::new();

    let messages_a = vec![user_msg("data for tenant A")];
    let trace: Vec<ReActStep> = vec![];
    let snapshot_a = make_snapshot(
        session_id,
        tenant_a,
        &messages_a,
        &trace,
        &GuardConfig::default(),
    );

    repo.save(session_id, tenant_a, snapshot_a)
        .await
        .expect("failed to save for tenant A");

    // Load with wrong tenant_id should fail (return None)
    let wrong_tenant = repo
        .load(session_id, tenant_b)
        .await
        .expect("load should not error");
    assert!(
        wrong_tenant.is_none(),
        "loading with wrong tenant_id should return None"
    );

    // Load with correct tenant_id should succeed
    let correct_tenant = repo
        .load(session_id, tenant_a)
        .await
        .expect("load should not error")
        .expect("session should exist for correct tenant");
    assert_eq!(correct_tenant.tenant_id, tenant_a);
}

// ── Test: update_status state transition ────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn update_status_state_transition() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();

    // Create a session initially as idle
    let snapshot = SessionSnapshot {
        session_id,
        tenant_id,
        status: SessionStatus::Idle,
        messages_json: "[]".to_string(),
        trace_json: "[]".to_string(),
        guard_config_json: "{}".to_string(),
        elapsed_ms: 0,
        iteration_count: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        termination_reason_json: None,
    };
    repo.save(session_id, tenant_id, snapshot)
        .await
        .expect("failed to save");

    // Transition: idle -> running
    repo.update_status(session_id, tenant_id, SessionStatus::Running)
        .await
        .expect("status update failed");

    let loaded = repo
        .load(session_id, tenant_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, SessionStatus::Running);

    // Transition: running -> ended
    repo.update_status(session_id, tenant_id, SessionStatus::Ended)
        .await
        .expect("status update failed");

    let loaded = repo
        .load(session_id, tenant_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, SessionStatus::Ended);
}

// ── Test: list_by_tenant returns summaries ──────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn list_by_tenant_returns_summaries() {
    let repo = make_repo().await;
    let tenant_id = TenantId::new();

    // Create 3 sessions
    for i in 0..3 {
        let session_id = SessionId::new();
        let messages = vec![user_msg(&format!("msg {}", i)), assistant_msg("reply")];
        let snapshot = make_snapshot(
            session_id,
            tenant_id,
            &messages,
            &vec![],
            &GuardConfig::default(),
        );
        repo.save(session_id, tenant_id, snapshot)
            .await
            .expect("failed to save");
    }

    let summaries = repo
        .list_by_tenant(tenant_id)
        .await
        .expect("list_by_tenant failed");
    assert_eq!(summaries.len(), 3, "should return 3 session summaries");

    // Each summary should have derived message_count
    for s in &summaries {
        assert_eq!(s.message_count, 2, "each session has 2 messages");
        assert_eq!(s.turn_count, 0, "each session has 0 trace steps");
        assert_eq!(s.tenant_id, tenant_id);
    }
}

// ── Test: elapsed_ms is preserved across save/load ──────────────────

#[tokio::test(flavor = "multi_thread")]
async fn elapsed_ms_preserved() {
    let repo = make_repo().await;
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();

    let snapshot = SessionSnapshot {
        session_id,
        tenant_id,
        status: SessionStatus::Paused,
        messages_json: "[]".to_string(),
        trace_json: "[]".to_string(),
        guard_config_json: "{}".to_string(),
        elapsed_ms: 42_000, // 42 seconds accumulated
        iteration_count: 5,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        termination_reason_json: None,
    };

    repo.save(session_id, tenant_id, snapshot)
        .await
        .expect("failed to save");

    let loaded = repo
        .load(session_id, tenant_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(loaded.elapsed_ms, 42_000);
    assert_eq!(loaded.iteration_count, 5);
}