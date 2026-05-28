//! # jadepaw-bus
//!
//! Message bus for intra-node and inter-node event routing between agents, tools,
//! and system components.
//!
//! ## What lives here
//!
//! - Typed event definitions (AgentEvent, ToolEvent, SystemEvent)
//! - Sharded local broadcast via tokio::broadcast per tenant
//! - Cross-node relay bridge (Redis PubSub for cluster mode)
//! - Skill DAG message passing infrastructure
//!
//! ## What does NOT live here
//!
//! - Agent orchestrator logic (see jadepaw-agent)
//! - HTTP routing or session affinity (see jadepaw-gateway)
//! - Wasm runtime or instance management (see jadepaw-wasm)
