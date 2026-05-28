//! # jadepaw-gateway
//!
//! HTTP gateway: routing, WebSocket upgrade, session affinity, authentication
//! middleware, and SSE streaming.
//!
//! ## What lives here
//!
//! - Session ID extraction and node/instance routing
//! - WebSocket upgrade and bidirectional streaming
//! - API Key / JWT middleware with tenant identity extraction
//! - Session registry (session-to-instance mapping)
//! - SSE token streaming for LLM output
//! - CORS, compression, and static file serving
//!
//! ## What does NOT live here
//!
//! - Agent loop or LLM orchestration (see jadepaw-agent)
//! - Wasm engine or instance pool (see jadepaw-wasm)
//! - Core data types (see jadepaw-core)
//! - Event bus routes (see jadepaw-bus)