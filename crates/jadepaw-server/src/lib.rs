//! # jadepaw-server
//!
//! Binary crate that wires all jadepaw library crates together into a running
//! server process. This crate contains startup, configuration loading, axum Router
//! assembly, middleware stack setup, and graceful shutdown orchestration.
//!
//! ## What lives here
//!
//! - `main.rs`: Startup, config loading, crate initialization order
//! - `app.rs`: axum Router assembly, middleware stack
//! - `shutdown.rs`: Graceful shutdown, pool drain, session migration
//!
//! ## What does NOT live here
//!
//! - Business logic — all library crates (core, wasm, agent, skill, gateway)
//! - The server crate is intentionally thin; its only role is dependency injection
//!   and process lifecycle management.

// D-04: Mutually exclusive feature guard — single-node and cluster deployment
// modes are mutually exclusive. This lives in the binary crate because workspace
// features cascade here, so both flags are visible when either is enabled.
#[cfg(all(feature = "single-node", feature = "cluster"))]
compile_error!("single-node and cluster modes are mutually exclusive");
