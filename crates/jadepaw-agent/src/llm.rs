//! LLM integration via async-openai.
//!
//! Handles prompt construction, streaming chat completions, and ReAct response
//! parsing. Uses `Client<Box<dyn Config>>` directly per D-05/D-06 — no
//! `LlmClient` trait abstraction.
//!
//! # Design (D-05, D-06, D-07)
//!
//! - `build_initial_messages()` constructs the system + user message tuple
//! - `stream_llm_response()` creates a streaming chat completion and pushes
//!   tokens through the mpsc channel as `ReActStep::Thought` events
//! - `parse_next_action()` is a minimal parser that extracts ACTION / FINAL
//!   ANSWER from the LLM response text
//! - `REACT_SYSTEM_PROMPT` is the hardcoded ReAct system prompt used by
//!   `run_agent()` unless overridden by a Skill

use anyhow::Context;
use async_openai::{
    Client,
    config::Config,
    types::chat::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessage,
        ChatCompletionRequestUserMessage, CreateChatCompletionRequestArgs,
    },
};
use futures::StreamExt;
use jadepaw_core::ReActStep;
use tokio::sync::mpsc;

/// Hardcoded ReAct system prompt.
///
/// Instructs the LLM to follow the think-act-observe pattern. Tokens are
/// streamed as `ReActStep::Thought` events through the mpsc channel, and
/// the full response is parsed for ACTION / FINAL ANSWER directives.
pub const REACT_SYSTEM_PROMPT: &str = r#"You are a capable AI agent. Follow this reasoning pattern:

1. Reason step-by-step before taking any action. Think aloud about what you know, what you need to find out, and why.
2. If you need to use a tool, respond with:
   THOUGHT: <your reasoning about what you need to do and why>
   ACTION: <tool_name>(<args>)
3. When you have enough information to answer the user's question, respond with:
   THOUGHT: <your final reasoning>
   FINAL ANSWER: <your answer>
4. Never invent information you don't have. If you're unsure, ask clarifying questions or request the relevant tool.
5. Keep responses concise, focused, and actionable. Avoid unnecessary verbosity.

Important: Use EXACTLY the format shown above. The THOUGHT section MUST come before ACTION or FINAL ANSWER."#;

/// The parsed directive from an LLM response.
///
/// This is the LLM-specific parse output type, distinct from
/// `jadepaw_core::guest_exports::NextAction` which serves as the guest
/// decision-point interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmDirective {
    /// The LLM wants to invoke a tool.
    Act {
        /// The LLM's reasoning content from the THOUGHT section.
        thought: String,
        /// Tool name.
        tool: String,
        /// Tool arguments as a raw string (caller may JSON-parse).
        args: String,
    },
    /// The LLM has produced a final answer.
    Finish {
        /// The LLM's reasoning content from the THOUGHT section.
        thought: String,
        /// The final answer text.
        answer: String,
    },
    /// The LLM is still thinking (no ACTION or FINAL ANSWER directives found).
    ContinueThinking {
        /// The LLM's reasoning content from the THOUGHT section.
        thought: String,
    },
}

/// Build the initial conversation messages for a chat completion.
///
/// Constructs a system message from `system_prompt` and a user message
/// containing optional context followed by the user's input.
///
/// The optional `context` is prepended as "Context: <context>\n\n" before the
/// user message. If absent, only the user message is included.
pub fn build_initial_messages(
    system_prompt: &str,
    user_message: &str,
    context: Option<&str>,
) -> Vec<ChatCompletionRequestMessage> {
    let system_msg: ChatCompletionRequestMessage =
        ChatCompletionRequestSystemMessage::from(system_prompt).into();

    let user_text = match context {
        Some(ctx) => format!("Context: {}\n\nUser: {}", ctx, user_message),
        None => user_message.to_string(),
    };
    let user_msg: ChatCompletionRequestMessage =
        ChatCompletionRequestUserMessage::from(user_text).into();

    vec![system_msg, user_msg]
}

/// Stream an LLM chat completion response and accumulate the full text.
///
/// Creates a streaming chat completion request, iterates over the token
/// stream, and accumulates tokens into a single response string. Per-token
/// streaming to SSE consumers is deferred to the caller (`react_loop`),
/// which emits a single `ReActStep::Thought` event with the complete
/// response after parsing.
///
/// The `tx` parameter is retained as a passthrough to detect channel close
/// (graceful early termination if the SSE consumer disconnects). It is NOT
/// used to emit per-token events.
///
/// # Errors
///
/// Returns an error if:
/// - The chat completion request fails to build
/// - The streaming call fails
/// - Any chunk in the stream reports an error
pub async fn stream_llm_response(
    client: &Client<Box<dyn Config>>,
    messages: Vec<ChatCompletionRequestMessage>,
    model: &str,
    tx: &mpsc::Sender<ReActStep>,
) -> anyhow::Result<String> {
    let request = CreateChatCompletionRequestArgs::default()
        .model(model)
        .messages(messages)
        .build()
        .context("failed to build chat completion request")?;

    let mut stream = client
        .chat()
        .create_stream(request)
        .await
        .context("failed to create streaming chat completion")?;

    let mut full_content = String::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                for choice in response.choices {
                    if let Some(content) = choice.delta.content {
                        full_content.push_str(&content);
                    }
                }
                // Check if the receiver is still alive (SSE consumer connected).
                // If the channel is closed, stop streaming gracefully.
                if tx.is_closed() {
                    return Ok(full_content);
                }
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    Ok(full_content)
}

/// Parse the LLM response text to determine the next action.
///
/// This is a minimal parser. It scans the response for recognized directives:
///
/// - `FINAL ANSWER:` prefix -> `LlmDirective::Finish` (with thought content)
/// - `ACTION:` prefix -> `LlmDirective::Act` (with thought content)
/// - Otherwise -> `LlmDirective::ContinueThinking` (carries the full response as thought)
///
/// The THOUGHT: prefix is extracted and attached to each directive variant.
/// If no THOUGHT: prefix is found, the thought field defaults to the full
/// response text (or the part before the directive).
///
/// The parsing is case-insensitive for the directive prefix. The content after
/// the prefix is trimmed.
pub fn parse_next_action(response: &str) -> LlmDirective {
    let thought = extract_thought(response).unwrap_or_else(|| response.to_string());
    let response_upper = response.to_uppercase();

    // Check for FINAL ANSWER first (more specific)
    if let Some(pos) = response_upper.find("FINAL ANSWER:") {
        let answer = response[pos + "FINAL ANSWER:".len()..].trim().to_string();
        if !answer.is_empty() {
            return LlmDirective::Finish {
                thought,
                answer,
            };
        }
    }

    // Check for ACTION:
    if let Some(pos) = response_upper.find("ACTION:") {
        let action_str = response[pos + "ACTION:".len()..].trim();
        // Parse tool_name(args) format
        if let Some(paren_pos) = action_str.find('(') {
            let tool = action_str[..paren_pos].trim().to_string();
            let args_and_close = &action_str[paren_pos + 1..];
            if let Some(close_pos) = args_and_close.rfind(')') {
                let args = args_and_close[..close_pos].trim().to_string();
                if !tool.is_empty() {
                    return LlmDirective::Act {
                        thought,
                        tool,
                        args,
                    };
                }
            }
        }
        // Fallback: treat entire string as tool name with empty args
        let tool = action_str.trim().to_string();
        if !tool.is_empty() {
            return LlmDirective::Act {
                thought,
                tool,
                args: String::new(),
            };
        }
    }

    LlmDirective::ContinueThinking { thought }
}

/// Extract the THOUGHT content from a ReAct-formatted LLM response.
///
/// Looks for a line starting with `THOUGHT:` (case-insensitive) and returns
/// the text between THOUGHT: and the next directive (ACTION:, FINAL ANSWER:)
/// or end of response. Returns `None` if no THOUGHT prefix is found.
fn extract_thought(response: &str) -> Option<String> {
    let response_upper = response.to_uppercase();
    let thought_pos = response_upper.find("THOUGHT:")?;
    let thought_start = thought_pos + "THOUGHT:".len();

    // Determine where the thought content ends: at the start of the next directive
    // or the start position of the directive keyword that was matched
    let remainder = &response[thought_start..];
    let remainder_upper = &response_upper[thought_start..];

    let end_pos = remainder_upper
        .find("FINAL ANSWER:")
        .or_else(|| remainder_upper.find("ACTION:"));

    let thought_content = match end_pos {
        Some(end) => remainder[..end].trim().to_string(),
        None => remainder.trim().to_string(),
    };

    if thought_content.is_empty() {
        None
    } else {
        Some(thought_content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_final_answer() {
        let result = parse_next_action(
            "THOUGHT: I now have all the information.\nFINAL ANSWER: The capital of France is Paris.",
        );
        assert_eq!(
            result,
            LlmDirective::Finish {
                thought: "I now have all the information.".to_string(),
                answer: "The capital of France is Paris.".to_string()
            }
        );
    }

    #[test]
    fn parse_action_with_args() {
        let result = parse_next_action(
            "THOUGHT: I need to look up the weather.\nACTION: get_weather(location=\"Paris\", unit=\"celsius\")",
        );
        assert_eq!(
            result,
            LlmDirective::Act {
                thought: "I need to look up the weather.".to_string(),
                tool: "get_weather".to_string(),
                args: "location=\"Paris\", unit=\"celsius\"".to_string(),
            }
        );
    }

    #[test]
    fn parse_action_no_args() {
        let result = parse_next_action("THOUGHT: Need to think more.\nACTION: think");
        assert_eq!(
            result,
            LlmDirective::Act {
                thought: "Need to think more.".to_string(),
                tool: "think".to_string(),
                args: String::new(),
            }
        );
    }

    #[test]
    fn parse_continue_thinking() {
        let result = parse_next_action(
            "THOUGHT: I am not sure what tool to use yet. Let me think more carefully.",
        );
        assert_eq!(
            result,
            LlmDirective::ContinueThinking {
                thought: "I am not sure what tool to use yet. Let me think more carefully."
                    .to_string(),
            }
        );
    }

    #[test]
    fn parse_case_insensitive() {
        let result = parse_next_action("thought: reasoning\nfinal answer: Done.");
        assert_eq!(
            result,
            LlmDirective::Finish {
                thought: "reasoning".to_string(),
                answer: "Done.".to_string()
            }
        );
    }

    #[test]
    fn parse_empty_action_after_colon_is_continue() {
        let result = parse_next_action("ACTION: ");
        assert_eq!(
            result,
            LlmDirective::ContinueThinking {
                thought: "ACTION: ".to_string(),
            }
        );
    }

    #[test]
    fn build_messages_with_context() {
        let msgs = build_initial_messages(
            "You are a helpful assistant.",
            "What is the weather?",
            Some("The user is in Paris."),
        );
        assert_eq!(msgs.len(), 2);
        // First message is system
        match &msgs[0] {
            ChatCompletionRequestMessage::System(s) => {
                assert!(format!("{:?}", s).contains("helpful assistant"));
            }
            _ => panic!("expected System message"),
        }
        // Second message is user with context
        match &msgs[1] {
            ChatCompletionRequestMessage::User(u) => {
                let debug_str = format!("{:?}", u);
                assert!(debug_str.contains("Context: The user is in Paris"));
                assert!(debug_str.contains("What is the weather?"));
            }
            _ => panic!("expected User message"),
        }
    }

    #[test]
    fn build_messages_without_context() {
        let msgs = build_initial_messages("You are helpful.", "Hello!", None);
        assert_eq!(msgs.len(), 2);
        match &msgs[1] {
            ChatCompletionRequestMessage::User(u) => {
                let debug_str = format!("{:?}", u);
                assert!(!debug_str.contains("Context:"));
                assert!(debug_str.contains("Hello!"));
            }
            _ => panic!("expected User message"),
        }
    }
}