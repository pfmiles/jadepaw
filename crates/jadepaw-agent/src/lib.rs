//! # jadepaw-agent
//!
//! Agent runtime: hybrid planning loop (coarse-grained plan + ReAct execution),
//! tool management, memory management, and LLM client integration.
//!
//! ## What lives here
//!
//! - Top-level session orchestrator managing loop lifecycle
//! - LLM-driven plan generation with deviation-triggered re-planning
//! - ReAct step executor: think -> tool -> observe -> decide
//! - Tool registry with MCP-compatible protocol adapter
//! - Short-term memory (window compression) and long-term memory (vector DB)
//!
//! ## What does NOT live here
//!
//! - Wasm instance pool or engine management (see jadepaw-wasm)
//! - HTTP/WS transport or session affinity (see jadepaw-gateway)
//! - Core data types (see jadepaw-core)
//! - Skill format or compilation (see jadepaw-skill)