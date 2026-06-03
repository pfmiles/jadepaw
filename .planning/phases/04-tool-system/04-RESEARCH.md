# Phase 04: Tool System - Research

**Researched:** 2026-06-03
**Domain:** Tool/function-calling infrastructure with MCP-compatible wire protocol, ReAct loop integration, SSRF-protected HTTP client
**Confidence:** HIGH

## Summary

Phase 04 replaces the Phase 3 placeholder observation in `react_loop()` with real tool execution. Tools are registered as `Arc<dyn Tool>` impls in a `ToolRegistry` (in `jadepaw-agent`), dispatched from the ReAct loop's `LlmDirective::Act { tool, args }` branch, and produce structured `ToolResult` values that become `ReActStep::Observation` entries in the execution trace.

The MCP wire format (tools/list, tools/call) is implemented internally without external dependencies — `serde_json::Value` carries `inputSchema`, and MCP-style error handling uses a two-tier model (protocol errors + tool execution errors with `isError`).

The `http_request_host_fn` stub from Phase 2 is filled in with real HTTP logic using `reqwest` (already transitive via async-openai, added as direct dependency in `jadepaw-wasm`). SSRF protection uses `tokio::net::lookup_host` to resolve the target hostname, then blocks all resolved IPs in private/loopback/link-local/multicast/broadcast/unspecified ranges using stable `std::net` methods available in Rust 1.85+.

**Primary recommendation:** Implement `Tool` trait + `ToolResult` in `jadepaw-core`, `ToolRegistry` in `jadepaw-agent`, wire the `LlmDirective::Act` branch to dispatch through the registry, implement the HTTP tool with SSRF protection in `jadepaw-wasm`, and add the `is_error` field to `ReActStep::Observation`.

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Registry + capability-aware dispatch. `Tool` trait in `jadepaw-core` with `name()`, `description()`, `input_schema()`, `call(args, session_handle) -> Result<ToolResult>`. `ToolRegistry` in `jadepaw-agent` holds `HashMap<ToolId, Arc<dyn Tool>>`. `can_call_tool()` on `SessionState` is the authoritative capability gate.
- **D-01a:** `Tool` trait and `HostFunctions` trait have separate concerns — `Tool` is agent-level, `HostFunctions` is Wasm-level. Wasm host functions are wrapped as `Tool` impls.
- **D-01b:** Wasm Linker's host functions remain available for direct guest calls. Agent loop dispatches through Registry, not guest. Guest `select_tool` becomes advisory.
- **D-02:** MCP-compatible wire format, implemented internally. `tools/list` returns `Vec<{name, description, inputSchema}>`. `tools/call` returns `{content, isError}`. No external MCP server/rmcp dependency in Phase 4.
- **D-02a:** No new external dependencies for MCP. `serde_json::Value` carries `inputSchema`. Minimal JSON-RPC handling.
- **D-03:** Domain whitelist + IP-layer check. `can_network_to` domain whitelist is primary defense. Added IP-layer validation via `tokio::net::lookup_host`, reject private/loopback/link-local/multicast IPs using `std::net`. DNS rebinding accepted as documented known risk for MVP.
- **D-03a:** Use `reqwest` added as direct dependency in `jadepaw-wasm`. Support GET/POST/PUT/PATCH/DELETE. Follow at most 1 redirect via `redirect::Policy::limited(1)`. Buffer response body with 1MB cap. Enforce 30s timeout via `tokio::time::timeout`. TLS verification enabled by default.
- **D-03b:** No rate limiting in Phase 4 MVP. `TenantQuotaLimiter` from Phase 2 covers aggregate budgets.
- **D-04:** Structured `ToolResult` enum: `Ok { data: serde_json::Value }` and `Error { code: String, message: String, retryable: bool }`. Maps to MCP's two-tier error model.
- **D-04a:** `ReActStep::Observation` gains `is_error: bool` field (default `false`). `result` string formatted with LLM-actionable error messaging.
- **D-04b:** Host functions at Wasm FFI boundary still return `i32` (-1 on error). `Tool` trait impl wraps this into `ToolResult::Error` with structured context.

### Claude's Discretion

No areas were deferred to Claude — all decisions were user-directed.

### Deferred Ideas (OUT OF SCOPE)

- Full MCP client (rmcp) — deferred to Phase 6+
- DNS rebinding defense (hickory-resolver or IP pinning) — documented known risk
- Per-tool rate limiting — deferred
- Tool output streaming — buffered responses only for MVP
- Tool timeout per-invocation — global 30s HTTP timeout sufficient for MVP

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| AGENT-02 | 支持工具/函数调用，工具通过 MCP 兼容协议注册，MVP 至少支持文件读写和 HTTP 请求 | Sections: Standard Stack (Tool, ToolRegistry, ToolResult), Architecture Patterns (Tool dispatch from ReAct loop), MCP wire format, HTTP tool with SSRF, File tool wrappers |

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Tool trait definition + ToolResult/ToolDefinition types | jadepaw-core | — | Zero-dep shared types; used by jadepaw-agent (registry) and jadepaw-wasm (impls) |
| ToolRegistry (registration, lookup, dispatch) | jadepaw-agent (host) | — | Agent-level orchestration; not a Wasm concern |
| Tool dispatch from ReAct loop | jadepaw-agent (host) | — | The react_loop() is the single integration point for replacing placeholder Observation |
| File tool (file_read, file_write) as Tool impl | jadepaw-wasm | — | Wasm host functions remain the sandboxed backend; Tool impl wraps them via SessionHandle |
| HTTP tool (http_request) as Tool impl | jadepaw-wasm | — | HTTP client logic + SSRF protection live in the Wasm crate because it uses SessionState capabilities |
| SSRF IP-layer validation | jadepaw-wasm (host) | — | Runs on the host side after tokio::net::lookup_host; blocks IPs in private/loopback/link-local/multicast ranges |
| reqwest HTTP client | jadepaw-wasm (dependency) | — | Added as direct dependency; already transitive via async-openai |
| Capability gate (can_call_tool) | jadepaw-wasm | jadepaw-agent | SessionState::can_call_tool() is the authoritative check; ToolRegistry calls it before dispatch |
| MCP wire format (tools/list, tools/call) | jadepaw-agent (ToolRegistry) | — | ToolRegistry provides MCP-compatible list/call methods; no separate MCP server |

## Standard Stack

### Core (New in Phase 4)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `reqwest` | 0.13.4 | HTTP client for `http_request` tool | Already transitive via async-openai 0.40. Added as direct dep in jadepaw-wasm. Mature, well-maintained, TLS defaults on. [VERIFIED: crates.io + slopcheck] |
| `tokio::net::lookup_host` | 1.52 (stdlib) | DNS resolution for SSRF IP check | Already available via `tokio = { features = ["full"] }`. Zero new dependency. [VERIFIED: docs.rs/tokio] |

### Existing (Already in Dependency Tree)

| Library | Version | Purpose | Why Relevant |
|---------|---------|---------|--------------|
| `serde_json` | 1.0 (workspace) | MCP inputSchema, ToolResult data, args serialization | Already in every crate. [VERIFIED: workspace Cargo.toml] |
| `dashmap` | 6 (workspace) | Concurrent ToolRegistry storage (`DashMap<ToolId, Arc<dyn Tool>>`) | Already used in jadepaw-wasm for InstancePool session tracking. Same pattern. [VERIFIED: workspace Cargo.toml] |
| `async-trait` | 0.1 | `Tool` trait async methods | Already used by `HostFunctions` trait in jadepaw-core. Same pattern. [VERIFIED: jadepaw-core Cargo.toml] |
| `tokio::sync::Semaphore` | 1.52 (stdlib) | Concurrency bounding (already in InstancePool) | Not new, but referenced for pattern consistency |
| `std::net::Ipv4Addr` | 1.85+ (stdlib) | SSRF IP classification (`is_private`, `is_loopback`, `is_link_local`, `is_multicast`, `is_broadcast`, `is_unspecified`) | All methods stabilized since Rust 1.7.0. Zero new dependencies for SSRF. [VERIFIED: doc.rust-lang.org/std/net] |
| `std::net::Ipv6Addr` | 1.85+ (stdlib) | SSRF IPv6 classification (`is_unique_local` since 1.84.0, `is_unicast_link_local` since 1.84.0, `is_loopback`, `is_multicast`, `is_unspecified`) | All methods available in Rust 1.85+. [VERIFIED: doc.rust-lang.org/std/net] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `ToolRegistry` with `DashMap` | `HashMap<RwLock<...>>` | DashMap provides lock-free concurrent reads without explicit lock management. Already proven pattern in InstancePool. |
| `reqwest` | `hyper` directly | reqwest is already in the dep tree (via async-openai). hyper would require manual TLS, redirect, cookie handling. reqwest handles all of this with safe defaults. |
| `tokio::net::lookup_host` for SSRF | `hickory-resolver` | hickory-resolver is more robust for DNS rebinding but adds a new dependency. Deferred to post-MVP per D-03. |
| `std::net` IP checks | Manual CIDR matching | `Ipv4Addr::is_private()` etc. are in std since Rust 1.7.0. Manual matching would be error-prone and less auditable. |
| MCP via `rmcp` crate | Internal implementation | Per D-02a, no new external deps for MCP. The wire format is simple enough (two JSON-RPC methods) to implement internally. rmcp is deferred to Phase 6+. |

**Installation (jadepaw-wasm Cargo.toml addition):**
```toml
reqwest = { version = "0.13", default-features = false, features = ["rustls-tls"] }
```

Using `rustls-tls` instead of `native-tls` to avoid OpenSSL system dependency — aligns with the project's Rust-centric philosophy. The `default-features = false` strips the default `native-tls` backend.

**Version verification:**
- `reqwest` 0.13.4 confirmed in Cargo.lock (transitive via async-openai 0.40) [VERIFIED: Cargo.lock]
- `dashmap` 6.x confirmed in workspace Cargo.toml [VERIFIED: workspace Cargo.toml]
- `serde_json` 1.0 confirmed in workspace Cargo.toml [VERIFIED: workspace Cargo.toml]

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| `reqwest` | crates.io | ~9 yrs | 150M+ total | github.com/seanmonstar/reqwest | [OK] | Approved |
| `dashmap` | crates.io | ~6 yrs | 50M+ total | github.com/xacrimon/dashmap | [OK] | Approved |
| `serde_json` | crates.io | ~9 yrs | 300M+ total | github.com/serde-rs/json | [OK] | Approved (already in tree) |
| `async-trait` | crates.io | ~6 yrs | 200M+ total | github.com/dtolnay/async-trait | [OK] | Approved (already in tree) |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

**Cross-ecosystem verification:** `reqwest` was checked on crates.io (Rust ecosystem) via `slopcheck install reqwest`. An npm package named `reqwest` also exists (v2.0.5) but is a completely different library — the Rust crate is the correct target. This is the expected cross-ecosystem pattern.

## Architecture Patterns

### System Architecture Diagram

```
                          Agent Request
                               |
                               v
                      +------------------+
                      |   run_agent()    |  (jadepaw-agent/src/lib.rs)
                      +------------------+
                               |
                               v
                      +------------------+
                      |  react_loop()    |  (jadepaw-agent/src/loop.rs)
                      +------------------+
                               |
                    LLM produces directive
                               |
                 +-------------+-------------+
                 |             |             |
            Finish        Act {tool,args}  ContinueThinking
                 |             |             |
                 v             v             v
            Return trace   +-----------+   Append to history
                           |ToolRegistry|  (jadepaw-agent)
                           | .call_tool()|
                           +-----------+
                                 |
                     +-----------+-----------+
                     |                       |
              can_call_tool()         lookup tool by name
              (capability gate)       in DashMap<ToolId, Arc<dyn Tool>>
                     |                       |
                     v                       v
              +-------------+        +------------------+
              | DENY -> Error|        | Arc<dyn Tool>    |
              | ToolResult   |        | .call(args,      |
              | {isError:true}|       |  session_handle) |
              +-------------+        +------------------+
                                              |
                              +---------------+---------------+
                              |               |               |
                         FileTool impl   HttpTool impl   (future tools)
                              |               |
                              v               v
                      +-------------+  +-------------------+
                      |SessionHandle|  | reqwest::Client   |
                      | -> Wasm host|  | -> SSRF check     |
                      |    functions|  | -> HTTP request   |
                      +-------------+  +-------------------+
                              |               |
                              v               v
                         ToolResult       ToolResult
                              |               |
                              +-------+-------+
                                      |
                                      v
                            ReActStep::Observation {
                              result: String,
                              is_error: bool,
                            }
                                      |
                                      v
                            Appended to trace
                            + message history
                            + sent via SSE channel
```

### Recommended Project Structure

```
crates/
  jadepaw-core/src/
    tool.rs              # NEW: Tool trait, ToolResult enum, ToolDefinition struct
    agent_types.rs       # MODIFIED: ReActStep::Observation gains is_error: bool
    host_functions.rs    # MODIFIED: HostFunctions gains http_request method
    lib.rs               # MODIFIED: re-export new tool types
  jadepaw-agent/src/
    tool_registry.rs     # NEW: ToolRegistry with DashMap<ToolId, Arc<dyn Tool>>
    loop.rs              # MODIFIED: LlmDirective::Act branch dispatches via ToolRegistry
    lib.rs               # MODIFIED: re-export ToolRegistry, wire into run_agent()
    llm.rs               # MODIFIED: REACT_SYSTEM_PROMPT augmented with tool list
  jadepaw-wasm/src/
    host/
      network.rs         # MODIFIED: http_request_host_fn stub replaced with real HTTP + SSRF
      filesystem.rs      # UNCHANGED (Tool trait wraps these, no internal change)
    tool_impls/
      mod.rs             # NEW: module for Tool trait impls
      file_tool.rs       # NEW: FileReadTool, FileWriteTool wrapping Wasm host fns
      http_tool.rs       # NEW: HttpRequestTool using reqwest + SSRF
    linker.rs            # UNCHANGED (host function registration)
    session.rs           # UNCHANGED (SessionHandle already exposes store/store_mut/instance)
    Cargo.toml           # MODIFIED: add reqwest direct dependency
```

### Pattern 1: Tool Trait in jadepaw-core (Types in core, impls downstream)

**What:** The `Tool` trait defines the agent-level tool abstraction. It lives in `jadepaw-core` with zero jadepaw-internal dependencies so all crates can reference it.

**When to use:** Every tool the agent can invoke MUST implement this trait. File tools, HTTP tools, and future tools (database, search, etc.) all use this same interface.

**Example:**
```rust
// crates/jadepaw-core/src/tool.rs
// Source: Derived from CONTEXT.md D-01, aligned with MCP tool schema

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Structured result from a tool invocation.
///
/// Maps to MCP's two-tier error model:
/// - Ok: successful execution (maps to MCP `isError: false`)
/// - Error: tool execution error (maps to MCP `isError: true`)
/// Protocol errors (capability denied, unknown tool) are handled at the
/// Registry level and do not go through ToolResult.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    Ok {
        /// The tool's output data as a JSON value.
        data: serde_json::Value,
    },
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
    pub fn to_observation_string(&self) -> String {
        match self {
            Self::Ok { data } => {
                serde_json::to_string_pretty(data)
                    .unwrap_or_else(|_| format!("{:?}", data))
            }
            Self::Error { code, message, retryable } => {
                let retry_hint = if *retryable {
                    " You may retry with different arguments."
                } else {
                    ""
                };
                format!(
                    "Error: {} (code: {}).{} Suggested: {}",
                    message, code, retry_hint,
                    // Provide LLM-actionable suggestion based on error code
                    match code.as_str() {
                        "NOT_FOUND" => "check the path/URL exists and try again.",
                        "TIMEOUT" => "the operation timed out. Try with a smaller scope.",
                        "HTTP_500" => "the remote server returned an error. Try again later.",
                        "CAPABILITY_DENIED" => "you do not have permission for this operation.",
                        _ => "review the error and adjust your approach.",
                    }
                )
            }
        }
    }

    /// Whether this result represents an error.
    /// Maps directly to MCP's `isError` field.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// MCP-compatible tool metadata.
///
/// Matches the MCP `tools/list` response item format:
/// `{ name, description, inputSchema }`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema describing the tool's parameters.
    /// Uses `serde_json::Value` — no external schema library needed.
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
    /// * `args` — JSON value matching the input_schema
    /// * `session_id` — the calling session (for logging/audit)
    ///
    /// # Returns
    /// `ToolResult::Ok` on success, `ToolResult::Error` on failure.
    async fn call(&self, args: serde_json::Value, session_id: crate::SessionId) -> ToolResult;

    /// Convenience: produce a full ToolDefinition for MCP tools/list.
    fn to_definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            input_schema: self.input_schema(),
        }
    }
}
```

### Pattern 2: ToolRegistry in jadepaw-agent (DashMap concurrent dispatch)

**What:** `ToolRegistry` is the single dispatch point for all tool invocations. It holds `DashMap<ToolId, Arc<dyn Tool>>` and provides MCP-compatible `list_tools()` and `call_tool()` methods. The capability check (`can_call_tool()`) is performed here.

**When to use:** The ReAct loop calls `registry.call_tool(name, args, session_handle)` when it encounters `LlmDirective::Act`. The `run_agent()` function receives the registry as a parameter.

**Example:**
```rust
// crates/jadepaw-agent/src/tool_registry.rs
// Source: CONTEXT.md D-01, D-01a, D-02; pattern from InstancePool DashMap usage

use std::sync::Arc;
use dashmap::DashMap;
use jadepaw_core::{ToolId, Tool, ToolResult, ToolDefinition, SessionId};
use jadepaw_wasm::SessionHandle;

/// Central registry for all tools available to the agent.
///
/// Thread-safe via DashMap. Provides MCP-compatible tools/list and tools/call
/// interfaces (D-02).
pub struct ToolRegistry {
    /// Tool ID to tool instance mapping.
    tools: DashMap<ToolId, Arc<dyn Tool>>,
    /// Tool name to Tool ID index for O(1) name lookup.
    name_index: DashMap<String, ToolId>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
            name_index: DashMap::new(),
        }
    }

    /// Register a tool in the registry.
    ///
    /// Returns the assigned ToolId. Panics if a tool with the same name
    /// is already registered (duplicate detection at registration time).
    pub fn register(&self, tool: Arc<dyn Tool>) -> ToolId {
        let id = ToolId::new();
        let name = tool.name().to_string();

        // Panic on duplicate name — registration is an initialization-time operation.
        // If a duplicate slips through, it's a programmer error that should fail fast.
        if self.name_index.contains_key(&name) {
            panic!("ToolRegistry: duplicate tool name '{}'", name);
        }

        self.name_index.insert(name, id);
        self.tools.insert(id, tool);
        id
    }

    /// MCP-compatible tools/list.
    ///
    /// Returns all registered tool definitions in MCP format.
    pub fn list_tools(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .map(|entry| entry.value().to_definition())
            .collect()
    }

    /// Look up a tool by name.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn Tool>> {
        let id = self.name_index.get(name)?;
        self.tools.get(&*id).map(|entry| Arc::clone(entry.value()))
    }

    /// MCP-compatible tools/call with capability enforcement (D-04, D-01).
    ///
    /// 1. Lookup tool by name
    /// 2. Capability check via `session.state().can_call_tool()`
    /// 3. Dispatch to `tool.call(args, session_id)`
    ///
    /// # Errors
    ///
    /// Returns `ToolResult::Error` for:
    /// - Unknown tool name (code: "UNKNOWN_TOOL", non-retryable)
    /// - Capability denied (code: "CAPABILITY_DENIED", non-retryable)
    /// Delegates all other errors to the tool implementation.
    pub async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        session: &SessionHandle,
    ) -> ToolResult {
        // Step 1: Lookup tool
        let tool = match self.get_by_name(name) {
            Some(t) => t,
            None => {
                return ToolResult::Error {
                    code: "UNKNOWN_TOOL".to_string(),
                    message: format!("Unknown tool: '{}'. Available tools: {:?}",
                        name,
                        self.name_index.iter().map(|e| e.key().clone()).collect::<Vec<_>>()),
                    retryable: false,
                };
            }
        };

        // Step 2: Capability check (D-01 — authoritative gate)
        // Find the ToolId for this tool name
        let tool_id = match self.name_index.get(name) {
            Some(id) => *id,
            None => {
                return ToolResult::Error {
                    code: "INTERNAL_ERROR".to_string(),
                    message: format!("Tool '{}' found in tools but not in name_index", name),
                    retryable: false,
                };
            }
        };

        {
            let state = session.store().data();
            if !state.can_call_tool(&tool_id) {
                return ToolResult::Error {
                    code: "CAPABILITY_DENIED".to_string(),
                    message: format!(
                        "Tool '{}' is not in the session's capability whitelist. \
                         Contact the administrator to grant this tool.",
                        name
                    ),
                    retryable: false,
                };
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
```

### Pattern 3: Tool Dispatch from ReAct Loop (Integration Point)

**What:** The `LlmDirective::Act { tool, args }` branch in `react_loop()` (currently lines 190-237 of `loop.rs`) is replaced to dispatch through the `ToolRegistry` instead of emitting a placeholder observation.

**Integration point (exact location):** `crates/jadepaw-agent/src/loop.rs` lines 218-228, replacing:
```rust
let observation = ReActStep::Observation {
    result: format!(
        "Tool '{}' called with args '{}'. Full tool execution is coming in Phase 4.",
        tool, args
    ),
};
```
with a registry dispatch that produces a `ToolResult`, formats it as an observation string, and uses the new `is_error` field.

**Key integration details:**
- The `react_loop()` signature gains a `tool_registry: &ToolRegistry` parameter.
- The `run_agent()` function creates (or receives) a `ToolRegistry` pre-populated with standard tools.
- The tool result's formatted string is appended to the LLM message history as a user message so the LLM can see tool outputs and adapt its next action.
- After dispatching a tool, the observation is pushed to the trace AND sent via the SSE channel.

### Pattern 4: HTTP Tool with SSRF Protection

**What:** The `HttpRequestTool` implements `Tool` and uses `reqwest` for actual HTTP requests. Before making any outbound request, it resolves the target hostname via `tokio::net::lookup_host` and checks every resolved IP against private/loopback/link-local/multicast/broadcast/unspecified ranges.

**Defense layers (defense-in-depth per D-03):**
1. Domain whitelist check (`can_access_domain()`) — already in Phase 2 stub, preserved
2. IP-layer check (SSRF) — new in Phase 4: resolve hostname, check all IPs
3. reqwest redirect policy: `Policy::limited(1)` — follows at most 1 redirect
4. Response body cap: 1MB via `take(1_048_576)` on the response stream
5. Timeout: 30s via `tokio::time::timeout`
6. TLS: enabled by default (reqwest default)

**SSRF blocked IP ranges (all checkable via stable `std::net` in Rust 1.85+):**

| Range | IPv4 Method | IPv6 Method |
|-------|-------------|-------------|
| `10.0.0.0/8` | `is_private()` | — |
| `172.16.0.0/12` | `is_private()` | — |
| `192.168.0.0/16` | `is_private()` | — |
| `127.0.0.0/8` | `is_loopback()` | `is_loopback()` |
| `169.254.0.0/16` | `is_link_local()` | — |
| `224.0.0.0/4` | `is_multicast()` | `is_multicast()` |
| `255.255.255.255` | `is_broadcast()` | — |
| `0.0.0.0` | `is_unspecified()` | `is_unspecified()` |
| `::1` | — | `is_loopback()` |
| `fe80::/10` | — | `is_unicast_link_local()` |
| `fc00::/7` | — | `is_unique_local()` |
| `ff00::/8` | — | `is_multicast()` |
| `::` | — | `is_unspecified()` |

**Example SSRF check function:**
```rust
// Source: CONTEXT.md D-03; doc.rust-lang.org/std/net for IP classification methods

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Check if an IP address is blocked for outbound SSRF.
///
/// Returns `true` if the IP is in a private, loopback, link-local,
/// multicast, broadcast, or unspecified range.
fn is_blocked_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            v4.is_private()       // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_loopback()    // 127.0.0.0/8
                || v4.is_link_local()  // 169.254.0.0/16
                || v4.is_multicast()   // 224.0.0.0/4
                || v4.is_broadcast()   // 255.255.255.255
                || v4.is_unspecified() // 0.0.0.0
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()            // ::1
                || v6.is_unique_local()      // fc00::/7
                || v6.is_unicast_link_local() // fe80::/10
                || v6.is_multicast()         // ff00::/8
                || v6.is_unspecified()       // ::
        }
    }
}
```

### Pattern 5: FileTool Wrappers for Wasm Host Functions

**What:** `FileReadTool` and `FileWriteTool` implement the `Tool` trait by delegating to the existing Wasm host functions through `SessionHandle`. The Wasm sandbox (path validation, capability check) remains the security boundary.

These are registered in the `ToolRegistry` at agent startup alongside the `HttpRequestTool`. Their `call()` method invokes the Wasm guest's imported host function via the linker, reading/writing guest memory as needed.

### Anti-Patterns to Avoid

- **Bypassing the ToolRegistry**: Never call `tool.call()` directly from the ReAct loop. Always go through `registry.call_tool()` so the capability check fires. Direct calls would skip the `can_call_tool()` gate.
- **Putting Tool impls in jadepaw-agent**: Tool implementations that use Wasm internals (SessionHandle, guest memory) MUST live in `jadepaw-wasm`. The agent crate only holds the `ToolRegistry` and the `Tool` trait reference.
- **Skipping IP check when domain check passes**: The domain whitelist is the first line of defense, but a whitelisted domain (e.g., `api.example.com`) could resolve to a private IP via DNS rebinding. The IP check is defense-in-depth.
- **Using `reqwest::Client::default()` for all requests**: Always use a Builder with explicit redirect policy (`Policy::limited(1)`) and timeout. The default client follows up to 10 redirects and has no timeout.
- **Adding `is_error` to `ReActStep::Observation` without backward compatibility**: Always default `is_error` to `false` in serde deserialization. The SSE stream consumer in Phase 7 checks this field.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| HTTP request execution | Manual `tokio::net::TcpStream` + TLS + HTTP parsing | `reqwest` 0.13 (already transitive) | TLS, redirects, connection pooling, cookie handling, HTTP/2, proxy support — reqwest handles all of this with safe defaults |
| Concurrent map for ToolRegistry | `HashMap<RwLock<...>>` | `DashMap` (already in workspace deps) | Lock-free concurrent reads, no explicit lock management, same pattern as InstancePool session tracking |
| SSRF IP range matching | Manual CIDR bit-masking | `std::net::Ipv4Addr::is_private()` etc. | stdlib methods are audited, RFC-compliant, and trivial to verify. Manual matching would be error-prone |
| JSON-RPC parsing for MCP | Full JSON-RPC framework | `serde_json::Value` + pattern matching | Two methods (tools/list, tools/call) don't justify a framework. MCP wire format is simple key-value JSON |
| Domain extraction from URL | Regex on URL string | Existing `extract_host_from_url()` in `network.rs` | Already implemented in Phase 2, tested, handles scheme/port/path stripping |

**Key insight:** Every "hand-roll" in this phase would introduce subtle security bugs (TLS misconfiguration, SSRF IP range off-by-one, redirect loop failure). The existing dependencies (reqwest, std::net, DashMap) are all in the dependency tree already or in std — adding them as direct dependencies is zero-cost in terms of supply chain surface.

## Runtime State Inventory

> Phase 04 is a greenfield feature addition (new `Tool` trait, `ToolRegistry`, HTTP tool), not a rename/refactor/migration phase. No runtime state migration is needed.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None — no databases or datastores reference tool-related keys | — |
| Live service config | None — no external services have Phase 4-specific config | — |
| OS-registered state | None — no OS-level registrations for tools | — |
| Secrets/env vars | None — tool system uses no secrets beyond what LLM calls already use | — |
| Build artifacts | None — new code only | — |

**Nothing found in any category — verified by reviewing all five categories against the Phase 4 feature scope.**

## Common Pitfalls

### Pitfall 1: Tool Call Without Capability Check

**What goes wrong:** The ReAct loop calls `tool.call()` directly, skipping `ToolRegistry::call_tool()` and its `can_call_tool()` check. A tool that should be denied for the session executes anyway.

**Why it happens:** The `Tool` trait's `call()` method is public and callable from anywhere. Developers might call it directly out of convenience.

**How to avoid:** Make `Tool::call()` doc-comment that it should only be invoked through `ToolRegistry::call_tool()`. In the ReAct loop, only use `registry.call_tool()`. Consider making `Tool::call()` take a private token type in a future phase.

**Warning signs:** `tool.call(args, session_id)` appearing outside of `tool_registry.rs`.

### Pitfall 2: Observation String Too Large for LLM Context

**What goes wrong:** An HTTP tool returns a 900KB response body, and the full body is formatted into the observation string. This bloats the LLM message history, wastes tokens, and may exceed context window limits.

**Why it happens:** The natural instinct is to include the full tool output in the observation.

**How to avoid:** Truncate observation strings to a reasonable limit (e.g., 50KB for text output). The full response body is available in `ToolResult::Ok { data }` for programmatic access, but the observation string fed to the LLM should be capped. Add `#[doc]` to `to_observation_string()` documenting this.

**Warning signs:** Observation strings exceeding 50KB in traces.

### Pitfall 3: Redirect Following SSRF Bypass

**What goes wrong:** `http://safe-domain.com` is whitelisted, but the server returns a 302 redirect to `http://169.254.169.254/latest/meta-data/`. If the redirect is followed without re-checking the IP, SSRF succeeds.

**Why it happens:** reqwest's default redirect policy follows up to 10 redirects. The domain whitelist check happens once, at the initial URL. Redirect targets are not re-validated.

**How to avoid:** Set `redirect::Policy::limited(1)` to follow at most 1 redirect. This is the minimum redirect support for practical use (many APIs redirect HTTP to HTTPS) while limiting the attack surface. The single redirect target's IP is still resolved and checked — SSRF protection applies at the connection layer, not just the initial URL.

**Warning signs:** Configuring reqwest without explicit redirect policy, or using `Policy::limited(n)` with `n > 1`.

### Pitfall 4: DNS Resolution Delay Blocking the Async Runtime

**What goes wrong:** `tokio::net::lookup_host` performs actual DNS queries which can take seconds for slow or malicious DNS servers. Without a timeout, this blocks the tool dispatch path.

**Why it happens:** DNS resolution in `tokio::net::lookup_host` uses the system resolver, which has its own timeout (often 5+ seconds). There is no built-in tokio timeout wrapper.

**How to avoid:** Wrap `tokio::net::lookup_host` in `tokio::time::timeout(Duration::from_secs(5), ...)`. If resolution times out, return `ToolResult::Error` with code "DNS_TIMEOUT".

**Warning signs:** `lookup_host` called without a `tokio::time::timeout` wrapper.

### Pitfall 5: ReActStep::Observation is_error Default

**What goes wrong:** The new `is_error` field on `ReActStep::Observation` breaks deserialization of existing traces or SSE events from Phase 3 that lack this field.

**Why it happens:** Adding a required field to a struct breaks backward compatibility with serde by default.

**How to avoid:** Use `#[serde(default)]` on the `is_error` field so it defaults to `false` when absent from serialized data. This maintains backward compatibility with Phase 3 traces.

**Warning signs:** Deserialization errors mentioning "missing field `is_error`".

## Code Examples

Verified patterns from official sources and existing codebase:

### ReActStep::Observation with is_error
```rust
// Source: CONTEXT.md D-04a; existing agent_types.rs structure
// Location: crates/jadepaw-core/src/agent_types.rs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReActStep {
    // ... existing variants unchanged ...
    Observation {
        /// The result returned by the tool (formatted for LLM consumption).
        result: String,
        /// Whether the observation represents an error.
        /// Defaults to false for backward compatibility with Phase 3 traces.
        #[serde(default)]
        is_error: bool,
    },
    // ... rest unchanged ...
}
```

### HostFunctions trait addition (additive-only)
```rust
// Source: CONTEXT.md D-01, D-03a; existing host_functions.rs pattern
// Location: crates/jadepaw-core/src/host_functions.rs

/// Execute an HTTP request on behalf of the guest.
///
/// Added in Phase 4 (additive — no existing implementations broken).
/// Returns (status_code, response_headers, response_body).
///
/// # Security
///
/// Implementations MUST enforce:
/// 1. Domain whitelist check (can_access_domain)
/// 2. IP-layer SSRF protection (block private/loopback/link-local/multicast)
/// 3. Redirect limit (at most 1)
/// 4. Response body cap (1MB)
/// 5. Timeout (30s)
async fn http_request(
    &self,
    method: String,
    url: String,
    headers: std::collections::HashMap<String, String>,
    body: Option<Vec<u8>>,
) -> Result<(u16, std::collections::HashMap<String, String>, Vec<u8>)>;
```

### reqwest Client with redirect + timeout config
```rust
// Source: docs.rs/reqwest/0.13.4/reqwest/struct.ClientBuilder.html
// Verified pattern for D-03a requirements

use reqwest::redirect;
use std::time::Duration;

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(redirect::Policy::limited(1))  // D-03a: at most 1 redirect
        .timeout(Duration::from_secs(30))         // D-03a: 30s total timeout
        .build()
        .expect("reqwest Client builder should not fail with valid config")
}
```

### tokio::net::lookup_host with timeout
```rust
// Source: docs.rs/tokio/latest/tokio/net/fn.lookup_host.html
// Verified pattern for D-03 SSRF hostname resolution

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

async fn resolve_and_check_ssrf(host: &str) -> Result<Vec<SocketAddr>, ToolResult> {
    let addrs: Vec<SocketAddr> = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host(format!("{}:0", host)),
    )
    .await
    .map_err(|_| ToolResult::Error {
        code: "DNS_TIMEOUT".to_string(),
        message: format!("DNS resolution timed out for host '{}'", host),
        retryable: true,
    })?
    .map_err(|e| ToolResult::Error {
        code: "DNS_ERROR".to_string(),
        message: format!("DNS resolution failed for host '{}': {}", host, e),
        retryable: true,
    })?
    .collect();

    // Check all resolved IPs for SSRF
    for addr in &addrs {
        if is_blocked_ip(&addr.ip()) {
            return Err(ToolResult::Error {
                code: "SSRF_BLOCKED".to_string(),
                message: format!(
                    "Host '{}' resolved to blocked IP address {} (private/loopback/link-local/multicast). \
                     Only public IP addresses are allowed.",
                    host, addr.ip()
                ),
                retryable: false,
            });
        }
    }

    Ok(addrs)
}
```

### LLM System Prompt Augmentation with Tool List
```rust
// Source: llm.rs REACT_SYSTEM_PROMPT; extended per MCP tools/list pattern
// The system prompt is augmented with available tool descriptions
// so the LLM knows what tools it can call.

fn build_system_prompt_with_tools(
    base_prompt: &str,
    tools: &[jadepaw_core::ToolDefinition],
) -> String {
    if tools.is_empty() {
        return base_prompt.to_string();
    }

    let tool_descriptions: Vec<String> = tools
        .iter()
        .map(|t| {
            format!(
                "- {}: {}\n  Parameters: {}",
                t.name,
                t.description,
                serde_json::to_string(&t.input_schema).unwrap_or_default()
            )
        })
        .collect();

    format!(
        "{}\n\nAvailable tools:\n{}\n\nWhen calling a tool, use the exact tool name and provide parameters as a JSON object.",
        base_prompt,
        tool_descriptions.join("\n")
    )
}
```

### ReAct Loop Integration (exact replacement)
```rust
// Source: loop.rs lines 190-237 (LlmDirective::Act branch)
// Location: crates/jadepaw-agent/src/loop.rs
// This replaces the Phase 3 placeholder observation

LlmDirective::Act { thought: _, tool, args } => {
    // Emit action step (unchanged from Phase 3)
    let parsed_args = match serde_json::from_str(&args) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(turn = turn, tool = %tool, raw_args = %args, error = %e,
                "failed to parse tool args as JSON, storing as raw string");
            serde_json::Value::String(args.clone())
        }
    };
    let action_step = ReActStep::Action {
        tool: tool.clone(),
        args: parsed_args.clone(),
    };
    trace.push(action_step.clone());
    if tx.send(action_step).await.is_err() {
        return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
    }

    // Phase 4: dispatch through ToolRegistry (replaces placeholder)
    let tool_result = tool_registry.call_tool(&tool, parsed_args, session).await;
    let is_error = tool_result.is_error();
    let result_str = tool_result.to_observation_string();

    let observation = ReActStep::Observation {
        result: result_str,
        is_error,
    };
    trace.push(observation.clone());
    if tx.send(observation).await.is_err() {
        return Err(loop_error(LoopErrorKind::ChannelClosed { turn }));
    }

    // Append tool result to LLM message history so the LLM can adapt
    let observation_msg: ChatCompletionRequestMessage =
        async_openai::types::chat::ChatCompletionRequestUserMessage::from(
            format!("Tool '{}' result:\n{}", tool, result_str),
        )
        .into();
    messages.push(observation_msg);
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Raw `extern "C"` FFI for guest-host | `func_wrap_async` on wasmtime Linker | Phase 2 | Async host functions required for Phase 3+ LLM streaming. Already adopted. |
| Placeholder observation string | Real tool dispatch via ToolRegistry | Phase 4 (this phase) | Agent can now take real actions. The LLM sees actual tool output and can adapt. |
| `http_request` stub returning -1 | Full reqwest-based HTTP with SSRF | Phase 4 (this phase) | Network capability becomes real. SSRF protection at IP layer. |
| `Ipv6Addr` methods on nightly only | `is_unique_local`, `is_unicast_link_local` stabilized in 1.84.0 | Rust 1.84.0 (2025-01-09) | Full IPv6 SSRF protection is available on stable Rust 1.85+ (the project minimum). [VERIFIED: doc.rust-lang.org/std/net/struct.Ipv6Addr.html] |

**Deprecated/outdated:**
- None applicable to Phase 4. All patterns are additive on existing code.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | `reqwest` 0.13.x uses hyper 1.x under the hood and the API for `ClientBuilder::redirect()` and `ClientBuilder::timeout()` is stable | Standard Stack | Low — reqwest is the most popular Rust HTTP client, stable API for years |
| A2 | The `Ipv4Addr::is_private()` method covers exactly `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16` | Architecture Patterns / SSRF | Low — documented in RFC 1918 and confirmed in Rust std docs |
| A3 | `DashMap` concurrent read performance is sufficient for the ToolRegistry use case (reads are far more frequent than writes) | Architecture Patterns / ToolRegistry | Low — same pattern already in use for InstancePool session tracking in Phase 2 |

**If this table has entries:** Three assumptions identified, all LOW risk. None require user confirmation before execution — they are standard library guarantees or ecosystem precedents.

## Open Questions

1. **ToolRegistry lifecycle: per-agent-run or global?**
   - What we know: CONTEXT.md doesn't specify. The hash map is thread-safe (DashMap), so it could be either shared across sessions or per-session.
   - What's unclear: Whether the same ToolRegistry instance is shared across all concurrent agent sessions or created fresh per `run_agent()` call.
   - Recommendation: Make `ToolRegistry` an `Arc<ToolRegistry>` shared across sessions. The DashMap is already concurrent-read-optimized, and tools are stateless (all state is in SessionHandle). This avoids per-session registration overhead and enables future hot-reload of tools.

2. **Should the HTTP tool handle non-200 status codes as errors?**
   - What we know: CONTEXT.md D-04 says HTTP 500 is a tool error. Success criteria #4 says HTTP 500 is reported as a structured error.
   - What's unclear: Whether 4xx errors (e.g., 404, 403) should be `ToolResult::Error` or `ToolResult::Ok` with the status code in data.
   - Recommendation: 4xx = `ToolResult::Ok` with status code in data (the tool succeeded at making the request; the response is valid data). 5xx = `ToolResult::Error` with retryable=true. This matches MCP semantics where `isError` signals tool-level failures, not application-level HTTP semantics.

3. **How should the REACT_SYSTEM_PROMPT be augmented with tool descriptions?**
   - What we know: The LLM needs to know available tools to produce well-formed ACTION directives. The LLM parsing in `parse_next_action()` currently handles `tool_name(args)` format.
   - What's unclear: Whether tool descriptions should be in the system prompt (static), injected as a user message per turn, or added as context.
   - Recommendation: Inject tool list into the system prompt at `build_initial_messages()` time. This is the standard MCP pattern — tools are discovered once and available for the session. The `run_agent()` function receives the `ToolRegistry`, calls `list_tools()`, and builds the augmented system prompt before the first LLM call.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| reqwest (crates.io) | HTTP tool implementation | Verified | 0.13.4 (in Cargo.lock) | Already transitive; adding as direct dep is zero-cost |
| tokio feature "net" | tokio::net::lookup_host for SSRF | Verified | 1.52 (features=["full"]) | Already enabled via "full" feature |
| Rust std::net (SSRF methods) | IP classification for SSRF | Verified | 1.85+ (project min) | All required methods stable: is_private (1.7.0), is_unique_local (1.84.0), is_unicast_link_local (1.84.0) |
| dashmap | ToolRegistry concurrent storage | Verified | 6.x (workspace dep) | Already in workspace Cargo.toml |
| serde_json | MCP inputSchema, ToolResult data | Verified | 1.0 (workspace dep) | Already in all crates |

**Missing dependencies with no fallback:** none
**Missing dependencies with fallback:** none

All dependencies are either already in the workspace or part of std. No new external crates need to be fetched beyond adding reqwest as a direct dependency (already in Cargo.lock as transitive).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust `#[test]` + `#[tokio::test(flavor = "multi_thread")]` |
| Config file | none — inline `#[cfg(test)]` modules and `tests/` integration test files |
| Quick run command | `cargo test -p jadepaw-core -p jadepaw-agent -p jadepaw-wasm` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGENT-02 (tool registration) | Register a file_read tool, query "read file /data/notes.txt and summarize", agent calls tool | integration | `cargo test -p jadepaw-agent -- test_agent_calls_file_read_tool` | No — Wave 0 |
| AGENT-02 (HTTP tool) | Register http_request tool, agent fetches public URL and processes response | integration | `cargo test -p jadepaw-agent -- test_agent_calls_http_tool` | No — Wave 0 |
| AGENT-02 (MCP wire format) | ToolRegistry::list_tools() returns MCP-compatible format; call_tool() accepts MCP-style args | unit | `cargo test -p jadepaw-agent -- test_registry_mcp_list_call` | No — Wave 0 |
| AGENT-02 (error handling) | Tool call that fails (file not found) produces structured error with is_error=true | unit | `cargo test -p jadepaw-core -- test_tool_result_error_format` | No — Wave 0 |
| AGENT-02 (error reporting to LLM) | Failed tool call produces LLM-actionable observation string | unit | `cargo test -p jadepaw-core -- test_tool_result_to_observation_error` | No — Wave 0 |
| SEC-06 (SSRF) | HTTP tool blocks requests to private IPs (10.x, 192.168.x, 127.x) | unit | `cargo test -p jadepaw-wasm -- test_ssrf_blocks_private_ips` | No — Wave 0 |
| SEC-06 (SSRF IPv6) | HTTP tool blocks requests to IPv6 loopback/link-local/unique-local | unit | `cargo test -p jadepaw-wasm -- test_ssrf_blocks_ipv6_private` | No — Wave 0 |
| SEC-06 (domain whitelist) | HTTP tool enforces can_access_domain capability | unit | `cargo test -p jadepaw-wasm -- test_http_tool_domain_whitelist` | No — Wave 0 |
| REACT-observation | ReActStep::Observation with is_error=false deserializes correctly from Phase 3 format | unit | `cargo test -p jadepaw-core -- test_observation_backward_compat` | No — Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p jadepaw-core -p jadepaw-agent -p jadepaw-wasm`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `crates/jadepaw-core/src/tool.rs` — new module, no test file yet. Needs: `test_tool_result_error_format`, `test_tool_result_to_observation_error`, `test_tool_definition_mcp_format`
- [ ] `crates/jadepaw-core/src/agent_types.rs` — needs `test_observation_backward_compat` (deserialize Observation without `is_error` field)
- [ ] `crates/jadepaw-agent/src/tool_registry.rs` — new module, no test file. Needs: `test_registry_register_lookup`, `test_registry_mcp_list_call`, `test_registry_duplicate_name_panics`, `test_registry_capability_denied`
- [ ] `crates/jadepaw-agent/tests/` — needs `tool_dispatch.rs` integration test: register file_read tool, go through react_loop(), verify trace contains real observation (not placeholder)
- [ ] `crates/jadepaw-wasm/src/tool_impls/` — new module, needs tests for FileReadTool, FileWriteTool, HttpRequestTool
- [ ] `crates/jadepaw-wasm/tests/` — needs `ssrf_protection.rs` integration test fixture
- [ ] `crates/jadepaw-core/Cargo.toml` — needs `serde` for `#[serde(default)]` on `is_error` (already present)

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | Tool dispatch uses existing session identity; no new auth surface |
| V3 Session Management | no | SessionHandle already carries session state from Phase 2 |
| V4 Access Control | yes | `can_call_tool()` on SessionState (Phase 2) + `can_access_domain()` (Phase 2). Defense-in-depth: capability check at Registry level AND Wasm host function entry. |
| V5 Input Validation | yes | Tool args are JSON-validated per tool's input_schema. HTTP URL/method/headers/body validated for bounds and format. File paths validated via existing sandbox path validation (Phase 2). |
| V6 Cryptography | no | TLS verification enabled by default in reqwest (no custom certs). No cryptographic operations in tool system. |
| V7 Error Handling | yes | Structured ToolResult errors with LLM-actionable messages. No stack traces or internal details leaked to LLM. Wasm FFI boundary errors (i32 -1) wrapped in typed ToolResult::Error. |

### Known Threat Patterns for Tool System

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Tool invocation without capability check | Elevation of Privilege | `ToolRegistry::call_tool()` gates every call on `can_call_tool()`. Direct `tool.call()` is never used in the ReAct loop. |
| SSRF via DNS rebinding to private IP | Information Disclosure | Domain whitelist (first line) + IP-layer check after `lookup_host` (second line). DNS rebinding is documented as known risk for MVP per D-03. |
| SSRF via HTTP redirect to private IP | Information Disclosure | `redirect::Policy::limited(1)` limits to 1 redirect. The IP of the redirect target is still checked by SSRF protection. |
| Malicious URL in tool args | Tampering | URL parsed and validated. Hostname extracted, resolved, IP-checked. Scheme restricted to http/https (reject file://, gopher://, etc.). |
| Overly large HTTP response exhausting memory | Denial of Service | Response body buffered with 1MB cap (`read().take(1_048_576)`). 30s timeout via `tokio::time::timeout`. |
| Tool name injection in LLM prompt | Spoofing | LLM can only invoke tools registered in the ToolRegistry. Unknown tool names produce `UNKNOWN_TOOL` error. The LLM cannot register new tools. |
| Path traversal in file tool args | Tampering | File tools delegate to Wasm host functions, which call `validate_sandbox_path()` (Phase 2, SEC-03). Path containment is enforced at the Wasm sandbox boundary. |

## Sources

### Primary (HIGH confidence)
- Official MCP specification (modelcontextprotocol.io/docs/concepts/tools) — tools/list JSON format, tools/call JSON format, isError field, two-tier error model, tool definition fields (name, description, inputSchema, annotations) [VERIFIED: WebFetch]
- docs.rs/reqwest/0.13.4/reqwest/struct.ClientBuilder.html — redirect policy, timeout configuration, TLS defaults, builder API [VERIFIED: WebFetch]
- docs.rs/reqwest/latest/reqwest/redirect/struct.Policy.html — Policy::limited(), Policy::none(), custom() API [VERIFIED: WebFetch]
- doc.rust-lang.org/std/net/struct.Ipv4Addr.html — is_private (RFC 1918), is_loopback, is_link_local, is_multicast, is_broadcast, is_unspecified, all stabilized since 1.7.0 [VERIFIED: WebFetch]
- doc.rust-lang.org/std/net/struct.Ipv6Addr.html — is_unique_local (1.84.0), is_unicast_link_local (1.84.0), is_loopback, is_multicast, is_unspecified [VERIFIED: WebFetch]
- docs.rs/tokio/latest/tokio/net/fn.lookup_host.html — async DNS resolution, returns `impl Iterator<Item = SocketAddr>` [VERIFIED: WebFetch]
- crate codebase — all canonical refs from CONTEXT.md read and analyzed [VERIFIED: codebase read]

### Secondary (MEDIUM confidence)
- CONTEXT.md D-01 through D-04b — all locked decisions verified against codebase reality [CITED: 04-CONTEXT.md]
- Cargo.lock — reqwest 0.13.4 confirmed as transitive dependency [VERIFIED: Cargo.lock grep]
- Workspace Cargo.toml — tokio features=["full"], dashmap workspace dep, serde_json workspace dep [VERIFIED: file read]
- Component Cargo.toml files — dependency structure verified for jadepaw-core, jadepaw-wasm, jadepaw-agent [VERIFIED: file read]

### Tertiary (LOW confidence)
- None — all findings verified against primary or secondary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all dependencies either already in workspace (dashmap, serde_json) or in Cargo.lock (reqwest 0.13.4). SSRF methods all stable on Rust 1.85+. Verified via docs.rs and doc.rust-lang.org.
- Architecture: HIGH — ToolRegistry pattern modeled on existing InstancePool DashMap usage. ReAct loop integration point is exact (loop.rs lines 218-228). All integration surfaces read and analyzed.
- Pitfalls: HIGH — five pitfalls identified, each with prevention strategy rooted in specific design decisions (D-01, D-03, D-04a).

**Research date:** 2026-06-03
**Valid until:** 2026-07-03 (stable domain, 30-day validity)

## Project Constraints (from CLAUDE.md)

| Constraint | Phase 04 Compliance |
|------------|---------------------|
| Rust + wasmtime + tokio (non-negotiable) | Compliant — all new code is Rust; reqwest works with tokio; wasmtime used via existing SessionHandle |
| Wasm hardware-level isolation | Compliant — file tools still route through Wasm sandbox; HTTP tool adds SSRF protection at host level |
| Multi-tenancy from Day 1 | Compliant — capability gates are per-session (SessionState); TenantQuotaLimiter covers aggregate budgets |
| Interface: built-in Web server (no CLI/Desktop) | Compliant — tool results stream via existing SSE channel to Phase 7 UI |
| License: open source, quality over speed | Compliant — no proprietary dependencies; reqwest is MIT/Apache-2.0 |
| Additive-only HostFunctions trait | Compliant — `http_request` method added, no existing methods removed |
| Types in core, impls downstream | Compliant — `Tool` trait + `ToolResult` in jadepaw-core; `ToolRegistry` in jadepaw-agent; Wasm-backed impls in jadepaw-wasm |
| Capability-gated before I/O | Compliant — `can_call_tool()` checked at Registry level AND at Wasm host function entry (defense-in-depth) |