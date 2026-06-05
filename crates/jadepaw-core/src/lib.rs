//! # jadepaw-core
//!
//! Core data types, error handling, and configuration primitives shared across all
//! jadepaw crates. This crate has zero internal jadepaw dependencies by design.
//!
//! ## What lives here
//!
//! - Shared types: SessionId, TenantId, ToolId, SkillId, CapabilitySet
//! - Unified error types and Result aliases
//! - Configuration structs (global, tenant, session layers)
//! - HostFunctions trait — canonical guest-host communication contract
//! - InstanceCapabilities — capability whitelist with default-deny semantics
//! - Tool trait, ToolResult, ToolDefinition — agent-level tool abstraction
//!
//! ## What does NOT live here
//!
//! - Wasm runtime logic (see jadepaw-wasm)
//! - Agent loop execution (see jadepaw-agent)
//! - HTTP/WS transport (see jadepaw-gateway)

pub mod agent_types;
pub mod capabilities;
pub mod error;
pub mod guest_exports;
pub mod host_functions;
pub mod skill_types;
pub mod tool;
pub mod types;

// Re-export all public types at crate root for convenient imports.
pub use agent_types::{AgentRequest, AgentResponse, AgentTerminationReason, ReActStep};
pub use capabilities::{DomainPattern, InstanceCapabilities, PathPattern};
pub use error::{JadepawError, Result};
pub use guest_exports::{GuestExports, NextAction, ToolChoice, ToolDef};
pub use host_functions::HostFunctions;
pub use tool::{extract_host_from_url, Tool, ToolDefinition, ToolResult};
pub use skill_types::{SkillId, SkillManifest, SkillValidationError};
pub use types::{SessionId, TenantId, ToolId};