//! `FileReadTool` and `FileWriteTool` — `Tool` trait implementations wrapping
//! the Wasm sandbox host functions.
//!
//! Per D-01a: `Tool` is the agent-level abstraction; `HostFunctions` is the
//! Wasm-level contract. These file tools reuse the existing Phase 2 Wasm sandbox
//! path validation (`validate_sandbox_path`) and capability checks — they do NOT
//! duplicate sandbox logic.
//!
//! Per D-04b: The Wasm host functions return `i32` (-1 on error). The Tool trait
//! impl wraps this into structured `ToolResult::Error` with LLM-actionable messages.
//!
//! # Security
//!
//! Path validation is performed by `validate_sandbox_path()` (SEC-03, T-04-09)
//! BEFORE any I/O. This is the same function used by `file_read_host_fn` and
//! `file_write_host_fn` in the Wasm boundary.

use std::path::PathBuf;

use async_trait::async_trait;
use jadepaw_core::{SessionId, Tool, ToolResult};
use serde_json::Value;

use crate::path::validate_sandbox_path;

/// Tool that reads file contents through the Wasm sandbox.
///
/// Reuses `validate_sandbox_path` from Phase 2 for path containment.
/// Implements the `Tool` trait for agent-level dispatch.
#[allow(dead_code)]
pub struct FileReadTool {
    /// Sandbox root directory for path containment.
    sandbox_root: PathBuf,
    /// Session identifier for logging/audit.
    session_id: SessionId,
}

impl FileReadTool {
    /// Create a new `FileReadTool` with the given sandbox root and session id.
    pub fn new(sandbox_root: PathBuf, session_id: SessionId) -> Self {
        Self {
            sandbox_root,
            session_id,
        }
    }
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &str {
        "file_read"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path. Returns the file contents as a string. \
         The file path is validated against the sandbox root directory."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read, relative to the sandbox root."
                }
            },
            "required": ["path"]
        })
    }

    async fn call(&self, args: Value, _session_id: SessionId) -> ToolResult {
        // 1. Extract and validate path argument
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::Error {
                    code: "INVALID_ARGS".to_string(),
                    message: "Missing required parameter: 'path'. Please provide a file path to read."
                        .to_string(),
                    retryable: false,
                };
            }
        };

        // 2. Path validation via the Phase 2 sandbox function (SEC-03, T-04-09)
        let safe_path = match validate_sandbox_path(path, &self.sandbox_root) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::Error {
                    code: "PATH_VALIDATION_ERROR".to_string(),
                    message: format!(
                        "Path validation failed for '{}': {}. Ensure the path is within the sandbox directory.",
                        path, e
                    ),
                    retryable: false,
                };
            }
        };

        // 3. Read file contents
        let contents = match tokio::fs::read(&safe_path).await {
            Ok(c) => c,
            Err(e) => {
                let code = if e.kind() == std::io::ErrorKind::NotFound {
                    "NOT_FOUND"
                } else {
                    "IO_ERROR"
                };
                let retryable = e.kind() != std::io::ErrorKind::NotFound;
                return ToolResult::Error {
                    code: code.to_string(),
                    message: format!(
                        "Error reading file at '{}': {}. Suggested: {}",
                        path,
                        e,
                        if e.kind() == std::io::ErrorKind::NotFound {
                            "check the file path exists and try again."
                        } else {
                            "the I/O operation failed; you may retry."
                        }
                    ),
                    retryable,
                };
            }
        };

        // 4. Convert bytes to UTF-8 string
        match String::from_utf8(contents) {
            Ok(content) => ToolResult::Ok {
                data: Value::String(content),
            },
            Err(_e) => ToolResult::Error {
                code: "INVALID_UTF8".to_string(),
                message: format!(
                    "File at '{}' is not valid UTF-8 text. Only text files can be read this way.",
                    path
                ),
                retryable: false,
            },
        }
    }
}

/// Tool that writes content to a file through the Wasm sandbox.
///
/// Reuses `validate_sandbox_path` from Phase 2 for path containment.
/// The file is created if it does not exist, truncated if it does.
#[allow(dead_code)]
pub struct FileWriteTool {
    /// Sandbox root directory for path containment.
    sandbox_root: PathBuf,
    /// Session identifier for logging/audit.
    session_id: SessionId,
}

impl FileWriteTool {
    /// Create a new `FileWriteTool` with the given sandbox root and session id.
    pub fn new(sandbox_root: PathBuf, session_id: SessionId) -> Self {
        Self {
            sandbox_root,
            session_id,
        }
    }
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &str {
        "file_write"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path. Creates the file if it doesn't exist, \
         overwrites if it does. The file path is validated against the sandbox root directory."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to write the file, relative to the sandbox root."
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn call(&self, args: Value, _session_id: SessionId) -> ToolResult {
        // 1. Extract and validate path argument
        let path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::Error {
                    code: "INVALID_ARGS".to_string(),
                    message: "Missing required parameter: 'path'. Please provide a file path to write."
                        .to_string(),
                    retryable: false,
                };
            }
        };

        // 2. Extract and validate content argument
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult::Error {
                    code: "INVALID_ARGS".to_string(),
                    message: "Missing required parameter: 'content'. Please provide the content to write."
                        .to_string(),
                    retryable: false,
                };
            }
        };

        // 3. Path validation via the Phase 2 sandbox function (SEC-03, T-04-09)
        let safe_path = match validate_sandbox_path(path, &self.sandbox_root) {
            Ok(p) => p,
            Err(e) => {
                return ToolResult::Error {
                    code: "PATH_VALIDATION_ERROR".to_string(),
                    message: format!(
                        "Path validation failed for '{}': {}. Ensure the path is within the sandbox directory.",
                        path, e
                    ),
                    retryable: false,
                };
            }
        };

        // 4. Write file contents
        match tokio::fs::write(&safe_path, content.as_bytes()).await {
            Ok(()) => ToolResult::Ok {
                data: Value::String(format!(
                    "Successfully wrote {} bytes to '{}'.",
                    content.len(),
                    path
                )),
            },
            Err(e) => ToolResult::Error {
                code: "IO_ERROR".to_string(),
                message: format!(
                    "Error writing file at '{}': {}. Suggested: check permissions and disk space, then retry.",
                    path, e
                ),
                retryable: true,
            },
        }
    }
}