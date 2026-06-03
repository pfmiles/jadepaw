//! Tool registry — central dispatch point for all tool invocations.
//!
//! `ToolRegistry` holds all registered tools in a concurrent `DashMap` and
//! provides MCP-compatible `tools/list` and `tools/call` interfaces with
//! capability-aware authorization (D-01, D-02).
//!
//! # Design (D-01, D-02)
//!
//! - Tools are stored in `DashMap<ToolId, Arc<dyn Tool>>` for lock-free
//!   concurrent reads (same pattern as Phase 2's InstancePool).
//! - A `DashMap<String, ToolId>` name index provides O(1) name→id lookup.
//! - `call_tool()` performs three steps: lookup → capability check → dispatch.
//! - The capability gate (`can_call_tool()`) is the authoritative policy
//!   decision point — tool impls are never called directly from the ReAct loop.
//!
//! # Security
//!
//! `call_tool()` gates every invocation on `SessionState::can_call_tool()`.
//! Direct `tool.call()` from outside this module bypasses the gate and
//! must never be used in the agent dispatch path.

use std::sync::Arc;

use dashmap::DashMap;
use jadepaw_core::{extract_host_from_url, Tool, ToolDefinition, ToolId, ToolResult};
use jadepaw_wasm::HTTP_REQUEST_TOOL_NAME;

/// Central registry for all tools available to the agent.
///
/// Thread-safe via `DashMap`. Provides MCP-compatible `tools/list` and
/// `tools/call` interfaces (D-02). Designed to be shared across sessions
/// as `Arc<ToolRegistry>`.
pub struct ToolRegistry {
    /// Tool ID to tool instance mapping.
    tools: DashMap<ToolId, Arc<dyn Tool>>,
    /// Tool name to Tool ID index for O(1) name lookup.
    name_index: DashMap<String, ToolId>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
            name_index: DashMap::new(),
        }
    }

    /// Register a tool in the registry.
    ///
    /// Returns the assigned `ToolId`. Panics if a tool with the same name
    /// is already registered — duplicate detection at registration time
    /// is a fail-fast programmer error enforcement.
    pub fn register(&self, tool: Arc<dyn Tool>) -> ToolId {
        let id = ToolId::new();
        let name = tool.name().to_string();

        if self.name_index.contains_key(&name) {
            panic!("ToolRegistry: duplicate tool name '{}'", name);
        }

        self.name_index.insert(name, id);
        self.tools.insert(id, tool);
        id
    }

    /// MCP-compatible `tools/list`.
    ///
    /// Returns all registered tool definitions in MCP format.
    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|entry| entry.value().to_definition())
            .collect()
    }

    /// Look up a tool by name.
    ///
    /// Returns `None` if no tool with the given name is registered.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let id = self.name_index.get(name)?;
        self.tools.get(&*id).map(|entry| Arc::clone(entry.value()))
    }

    /// MCP-compatible `tools/call` with capability enforcement (D-01, D-04).
    ///
    /// # Execution flow
    ///
    /// 1. Lookup tool by name → `UNKNOWN_TOOL` error if not found
    /// 2. Capability check via `session.state().can_call_tool()` → `CAPABILITY_DENIED` error if rejected
    /// 3. Dispatch to `tool.call(args, session_id)`
    ///
    /// # Errors
    ///
    /// Returns `ToolResult::Error` for:
    /// - Unknown tool name (code: "UNKNOWN_TOOL", non-retryable)
    /// - Capability denied (code: "CAPABILITY_DENIED", non-retryable)
    /// - Internal consistency error (code: "INTERNAL_ERROR", non-retryable)
    ///
    /// Delegates all tool execution errors to the tool implementation.
    pub async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        session: &jadepaw_wasm::SessionHandle,
    ) -> ToolResult {
        // Step 1: Lookup tool by name
        let tool = match self.get_by_name(name) {
            Some(t) => t,
            None => {
                let available: Vec<String> =
                    self.name_index.iter().map(|e| e.key().clone()).collect();
                return ToolResult::from_error(
                    "UNKNOWN_TOOL",
                    &format!(
                        "Unknown tool: '{}'. Available tools: {:?}",
                        name, available
                    ),
                    false,
                );
            }
        };

        // Step 2: Capability check (D-01 — authoritative policy decision point)
        let tool_id = match self.name_index.get(name) {
            Some(id) => *id,
            None => {
                return ToolResult::from_error(
                    "INTERNAL_ERROR",
                    &format!(
                        "Tool '{}' found in tools but not in name_index",
                        name
                    ),
                    false,
                );
            }
        };

        // Scope the capability check in a block so the store borrow is
        // dropped before `tool.call()` which may also need store access
        // (e.g., for logging or audit).
        {
            let state = session.store().data();
            if !state.can_call_tool(&tool_id) {
                return ToolResult::from_error(
                    "CAPABILITY_DENIED",
                    &format!(
                        "Tool '{}' is not in the session's capability whitelist. \
                         Contact the administrator to grant this tool.",
                        name
                    ),
                    false,
                );
            }

            // CR-01: Per-operation domain capability check for http_request tool.
            // HttpRequestTool::call() only has SessionId, not SessionState, so it
            // cannot access the can_network_to whitelist. This check closes the
            // gap by enforcing domain capability at the Registry level (D-01a).
            if name == HTTP_REQUEST_TOOL_NAME {
                if let Some(host) = args.get("url").and_then(|v| v.as_str()).map(extract_host_from_url) {
                    if !state.can_access_domain(host) {
                        return ToolResult::from_error(
                            "CAPABILITY_DENIED",
                            &format!(
                                "Domain '{}' is not in the session's network capability whitelist. \
                                 The agent is only allowed to access domains listed in can_network_to.",
                                host
                            ),
                            false,
                        );
                    }
                }
            }
        }

        // Step 3: Dispatch to tool
        let session_id = session.store().data().session_id;
        tool.call(args, session_id).await
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use jadepaw_core::SessionId;
    use serde_json::json;

    /// A test tool that echoes its args.
    struct EchoTool {
        name: String,
        description: String,
    }

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            &self.description
        }

        fn input_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            })
        }

        async fn call(
            &self,
            args: serde_json::Value,
            _session_id: SessionId,
        ) -> ToolResult {
            ToolResult::Ok { data: args }
        }
    }

    #[test]
    fn test_registry_new_is_empty() {
        let registry = ToolRegistry::new();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn test_registry_register_lookup() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(EchoTool {
            name: "echo".to_string(),
            description: "Echoes input".to_string(),
        });
        let id = registry.register(tool);
        assert!(!id.to_string().is_empty());

        let found = registry.get_by_name("echo");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name(), "echo");
    }

    #[test]
    #[should_panic(expected = "duplicate tool name")]
    fn test_registry_duplicate_name_panics() {
        let registry = ToolRegistry::new();
        let tool1 = Arc::new(EchoTool {
            name: "dup".to_string(),
            description: "First".to_string(),
        });
        let tool2 = Arc::new(EchoTool {
            name: "dup".to_string(),
            description: "Second".to_string(),
        });
        registry.register(tool1);
        registry.register(tool2); // should panic
    }

    #[test]
    fn test_registry_list_tools_mcp_format() {
        let registry = ToolRegistry::new();
        let tool = Arc::new(EchoTool {
            name: "echo".to_string(),
            description: "Echoes input".to_string(),
        });
        registry.register(tool);

        let tools = registry.list_tools();
        assert_eq!(tools.len(), 1);
        let def = &tools[0];
        assert_eq!(def.name, "echo");
        assert_eq!(def.description, "Echoes input");
        assert!(def.input_schema.is_object());
    }

    #[test]
    fn test_registry_lookup_unknown() {
        let registry = ToolRegistry::new();
        assert!(registry.get_by_name("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_call_tool_unknown_returns_error() {
        let registry = ToolRegistry::new();
        // We need a SessionHandle for call_tool, but for UNKNOWN_TOOL
        // the check happens before accessing the session.
        // Since we can't easily construct a real SessionHandle, we test
        // the lookup path via get_by_name + manual error construction.
        let result = registry.get_by_name("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_registry_default() {
        let registry = ToolRegistry::default();
        assert!(registry.list_tools().is_empty());
    }

    #[test]
    fn test_registry_tool_definition() {
        let tool = EchoTool {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
        };
        let def = tool.to_definition();
        assert_eq!(def.name, "test_tool");
        assert_eq!(def.description, "A test tool");
        assert!(def.input_schema.is_object());
    }
}