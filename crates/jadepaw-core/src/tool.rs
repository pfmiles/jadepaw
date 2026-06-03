//! Tool abstraction layer — the agent-level interface for tool invocation.
//!
//! Defines the `Tool` trait (what the ReAct loop dispatches), `ToolResult`
//! (structured success/error with LLM-actionable messages), and `ToolDefinition`
//! (MCP-compatible tool metadata format per D-02).
//!
//! # Design (D-01, D-04, D-04a)
//!
//! - `Tool` trait lives in `jadepaw-core` with zero jadepaw-internal dependencies
//!   so all crates can reference it.
//! - `ToolResult` implements MCP's two-tier error model: protocol errors
//!   (capability denied, path validation failure) vs tool execution errors
//!   (file not found, HTTP 500).
//! - `ToolDefinition` matches MCP's `{ name, description, inputSchema }` format
//!   for `tools/list` responses.
//!
//! # Security note
//!
//! `Tool::call()` should only be invoked through `ToolRegistry::call_tool()`.
//! Direct calls bypass the `can_call_tool()` capability gate.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::SessionId;

/// Extract the host portion from a URL string.
///
/// Handles both `http://example.com/path` and simple `example.com` forms.
/// Returns the domain without port, path, query, or fragment.
///
/// Used by both the agent-level `ToolRegistry` domain capability check and
/// the Wasm-level `HttpRequestTool` URL validation. A single canonical
/// implementation avoids duplicated string-processing logic.
pub fn extract_host_from_url(url: &str) -> &str {
    // Strip scheme
    let after_scheme = if let Some(idx) = url.find("://") {
        &url[idx + 3..]
    } else {
        url
    };

    // Strip userinfo (user:password@) — CR-01: prevent credential-based
    // domain whitelist bypass. A URL like
    // `http://whitelisted.com:pwd@evil.com/path` would otherwise return
    // the username as the "host", defeating the domain capability check.
    let after_userinfo = if let Some(idx) = after_scheme.find('@') {
        &after_scheme[idx + 1..]
    } else {
        after_scheme
    };

    // Strip path, query, fragment
    let host_and_port = if let Some(idx) = after_userinfo.find('/') {
        &after_userinfo[..idx]
    } else if let Some(idx) = after_userinfo.find('?') {
        &after_userinfo[..idx]
    } else if let Some(idx) = after_userinfo.find('#') {
        &after_userinfo[..idx]
    } else {
        after_userinfo
    };

    // Strip port
    if let Some(idx) = host_and_port.find(':') {
        &host_and_port[..idx]
    } else {
        host_and_port
    }
}

/// Structured result from a tool invocation.
///
/// Maps to MCP's two-tier error model:
/// - `Ok`: successful execution (maps to MCP `isError: false`)
/// - `Error`: tool execution error (maps to MCP `isError: true`)
///
/// Protocol errors (capability denied, unknown tool) are handled at the
/// Registry level and do not use ToolResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    /// The tool executed successfully.
    Ok {
        /// The tool's output data as a JSON value.
        data: serde_json::Value,
    },
    /// The tool encountered an execution error.
    Error {
        /// Machine-readable error code (e.g., "NOT_FOUND", "TIMEOUT", "HTTP_500").
        code: String,
        /// Human-readable error message for LLM consumption.
        message: String,
        /// Whether the LLM can retry with different arguments.
        retryable: bool,
    },
}

impl ToolResult {
    /// Format the result as an LLM-consumable observation string.
    ///
    /// For successful results, the data is pretty-printed as JSON.
    /// For errors, a structured message with an LLM-actionable suggestion
    /// is included based on the error code.
    ///
    /// Note: the caller should truncate the output to a reasonable size
    /// (e.g., 50KB) before appending to the LLM message history to avoid
    /// context window bloat.
    pub fn to_observation_string(&self) -> String {
        match self {
            Self::Ok { data } => serde_json::to_string_pretty(data)
                .unwrap_or_else(|_| format!("{:?}", data)),
            Self::Error {
                code,
                message,
                retryable,
            } => {
                let retry_hint = if *retryable {
                    " You may retry with different arguments."
                } else {
                    ""
                };
                format!(
                    "Error: {} (code: {}).{} Suggested: {}",
                    message,
                    code,
                    retry_hint,
                    match code.as_str() {
                        "NOT_FOUND" => "check the path/URL exists and try again.",
                        "TIMEOUT" => "the operation timed out. Try with a smaller scope.",
                        "HTTP_500" => "the remote server returned an error. Try again later.",
                        "CAPABILITY_DENIED" => {
                            "you do not have permission for this operation."
                        }
                        _ => "review the error and adjust your approach.",
                    }
                )
            }
        }
    }

    /// Whether this result represents an error.
    ///
    /// Maps directly to MCP's `isError` field. Returns `true` for
    /// `ToolResult::Error` and `false` for `ToolResult::Ok`.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Convenience constructor for creating an error result.
    pub fn from_error(code: &str, message: &str, retryable: bool) -> Self {
        Self::Error {
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        }
    }
}

/// MCP-compatible tool metadata.
///
/// Matches the MCP `tools/list` response item format:
/// `{ name, description, inputSchema }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique tool name (e.g., "file_read", "http_request").
    pub name: String,
    /// Human-readable description for LLM consumption.
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// The agent-level tool abstraction.
///
/// Separate concern from `HostFunctions` (D-01a):
/// - `Tool` = agent-level dispatch (ReAct loop calls this)
/// - `HostFunctions` = Wasm-level contract (guest-host FFI)
///
/// Wasm-backed tools implement `Tool` by wrapping `HostFunctions` calls
/// through `SessionHandle`.
///
/// # Security
///
/// `call()` should only be invoked through `ToolRegistry::call_tool()`.
/// Direct calls bypass the `can_call_tool()` capability gate.
///
/// # Limitation (WR-06)
///
/// The `call()` signature receives only `SessionId`, not `SessionState` or
/// `InstanceCapabilities`. This means per-operation capability enforcement
/// (domain whitelist, path patterns) MUST happen at the Registry level in
/// `ToolRegistry::call_tool()`, not inside individual `Tool` implementations.
/// See `tool_registry.rs` for the http_request domain check as an example.
/// A future refactor may extend the signature to accept capabilities directly.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g., "file_read", "http_request").
    fn name(&self) -> &str;

    /// Human-readable description for LLM consumption.
    fn description(&self) -> &str;

    /// JSON Schema defining the tool's input parameters.
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool with the given arguments.
    ///
    /// # Arguments
    ///
    /// * `args` — JSON value matching the input_schema
    /// * `session_id` — the calling session (for logging/audit)
    ///
    /// # Returns
    ///
    /// `ToolResult::Ok` on success, `ToolResult::Error` on failure.
    async fn call(&self, args: serde_json::Value, session_id: SessionId) -> ToolResult;

    /// Convenience: produce a full `ToolDefinition` for MCP `tools/list`.
    ///
    /// Default implementation assembles the definition from `name()`,
    /// `description()`, and `input_schema()`. Implementors typically do
    /// not need to override this.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_host_from_url tests ──────────────────────────────────

    #[test]
    fn extract_host_basic() {
        assert_eq!(extract_host_from_url("https://example.com/path"), "example.com");
    }

    #[test]
    fn extract_host_with_port() {
        assert_eq!(extract_host_from_url("http://localhost:8080/api"), "localhost");
    }

    #[test]
    fn extract_host_no_scheme() {
        assert_eq!(extract_host_from_url("api.example.com/v1"), "api.example.com");
    }

    #[test]
    fn extract_host_bare_domain() {
        assert_eq!(extract_host_from_url("example.com"), "example.com");
    }

    /// CR-01: userinfo portion must be stripped before host extraction
    /// to prevent domain whitelist bypass via URLs like
    /// `http://whitelisted.com:pwd@evil.com/path`.
    #[test]
    fn extract_host_with_userinfo() {
        assert_eq!(
            extract_host_from_url("http://user:pass@example.com/path"),
            "example.com"
        );
        assert_eq!(
            extract_host_from_url("http://whitelisted.com:secret@evil.com/api"),
            "evil.com"
        );
        assert_eq!(
            extract_host_from_url("https://user@example.com"),
            "example.com"
        );
    }

    }