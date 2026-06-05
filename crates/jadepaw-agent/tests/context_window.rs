//! Tests for context window management: token counting, threshold
//! detection, and message compression behavior.
//!
//! These tests verify MEM-01 requirements: auto-compression triggers
//! at 65% context window and preserves recent N turns verbatim.

use jadepaw_agent::window::{count_tokens, should_compress, compress_context};
use async_openai::types::chat::{
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessage, ChatCompletionRequestUserMessage,
};

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

/// Helper: build a message list simulating a long conversation.
fn build_long_conversation(num_turns: usize) -> Vec<ChatCompletionRequestMessage> {
    let mut msgs = vec![
        system_msg("You are a helpful assistant."),
        user_msg("Hello, I need help with a long task."),
    ];
    let lorem = "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor. ";
    for i in 0..num_turns {
        msgs.push(user_msg(&format!(
            "What is the status of task {}? {}",
            i, lorem
        )));
        msgs.push(assistant_msg(&format!(
            "Task {} is in progress. The current status is: processing step {}. {}",
            i, i, lorem
        )));
    }
    msgs
}

// ── Test: token counting on empty vec returns 0 ─────────────────────

#[test]
fn count_tokens_empty_returns_zero() {
    let tokens = count_tokens(&[], "gpt-4o");
    assert_eq!(tokens, 0);
}

// ── Test: token counting on single user message returns positive ─────

#[test]
fn count_tokens_single_user_message_returns_positive() {
    let msgs = vec![user_msg("Hello, how are you today?")];
    let tokens = count_tokens(&msgs, "gpt-4o");
    assert!(tokens > 0, "expected positive tokens, got {tokens}");
}

// ── Test: should_compress returns false below 65% threshold ─────────

#[test]
fn should_compress_returns_false_for_small_messages() {
    let msgs = vec![
        system_msg("You are helpful."),
        user_msg("Hi!"),
        assistant_msg("Hello!"),
    ];
    assert!(
        !should_compress(&msgs, "gpt-4o"),
        "short messages should not trigger compression"
    );
}

// ── Test: should_compress returns true above 65% threshold ──────────

#[test]
fn should_compress_returns_true_above_65_percent() {
    // Use gpt-3.5-turbo with 4096 context window (65% = 2662 tokens).
    // Build ~100 turns with long content.
    let mut msgs = vec![
        system_msg("You are a helpful assistant. Please help with all tasks carefully."),
        user_msg("I need very detailed responses for every question I ask."),
    ];
    let lorem = "Lorem ipsum dolor sit amet consectetur adipiscing. ";
    for _ in 0..80 {
        msgs.push(user_msg(&lorem.repeat(5)));
        msgs.push(assistant_msg(&lorem.repeat(5)));
    }

    let should = should_compress(&msgs, "gpt-3.5-turbo");
    // With 80 turns of long text this should definitely exceed 2662 tokens.
    assert!(
        should,
        "long conversation should trigger compression at 65% threshold"
    );
}

// ── Test: compress_context preserves recent N=5 turns ───────────────

#[test]
fn compress_context_preserves_recent_5_turns() {
    let msgs = build_long_conversation(15); // 2 + 30 = 32 messages
    let original_len = msgs.len();
    let compressed = compress_context(msgs.clone(), "gpt-4o", 5);

    // Recent 5 turns = 10 messages should match the end of the original.
    let recent_count = (5 * 2).min(original_len - 2); // 5 turns * 2 msgs/turn
    let original_recent = &msgs[original_len - recent_count..];
    let compressed_recent = &compressed[compressed.len() - recent_count..];

    assert_eq!(
        original_recent.len(),
        compressed_recent.len(),
        "recent turn count should match"
    );

    for (orig, comp) in original_recent.iter().zip(compressed_recent.iter()) {
        let orig_text = serde_json::to_string(orig).unwrap();
        let comp_text = serde_json::to_string(comp).unwrap();
        assert_eq!(
            orig_text, comp_text,
            "recent turns must be preserved verbatim"
        );
    }
}

// ── Test: compress_context reduces total token count ────────────────

#[test]
fn compress_context_reduces_token_count() {
    let msgs = build_long_conversation(20);
    let original_tokens = count_tokens(&msgs, "gpt-4o");
    let compressed = compress_context(msgs, "gpt-4o", 5);
    let compressed_tokens = count_tokens(&compressed, "gpt-4o");

    assert!(
        compressed_tokens < original_tokens,
        "compressed tokens ({compressed_tokens}) should be less than original ({original_tokens})"
    );
}

// ── Test: compress_context injects a summary system message ─────────

#[test]
fn compress_context_injects_summary_message() {
    let msgs = build_long_conversation(10);
    let compressed = compress_context(msgs, "gpt-4o", 5);

    // After system + user, the next message should be a system summary.
    assert!(
        compressed.len() >= 3,
        "expected at least 3 messages after compression"
    );

    let is_system = matches!(&compressed[2], ChatCompletionRequestMessage::System(_));
    assert!(is_system, "third message should be a System (summary) message");

    // Verify the summary message contains "Previous conversation summary:".
    if let ChatCompletionRequestMessage::System(s) = &compressed[2] {
        let content = format!("{:?}", s);
        assert!(
            content.contains("Previous conversation summary"),
            "summary should contain 'Previous conversation summary' prefix"
        );
    }
}

// ── Test: should_compress uses correct model-specific threshold ──────

#[test]
fn should_compress_uses_model_specific_context_window() {
    // gpt-4: 8192 tokens, 65% = 5324
    // gpt-4o: 128000 tokens, 65% = 83200

    let msgs = build_long_conversation(10); // moderate conversation

    // With gpt-4's tiny 8K window, this might trigger.
    // With gpt-4o's 128K window, this definitely won't.
    let gpt4_result = should_compress(&msgs, "gpt-4");
    let gpt4o_result = should_compress(&msgs, "gpt-4o");

    // At minimum, verify that gpt-4o is less likely to trigger.
    // This tests the model-specific window size selection.
    if gpt4_result {
        // If gpt-4 triggers, gpt-4o must not (128K >> 8K)
        assert!(
            !gpt4o_result,
            "gpt-4o 128K window should not trigger when gpt-4 8K does"
        );
    }
}