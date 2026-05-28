//! # jadepaw-wasm
//!
//! WebAssembly runtime integration: instance pool management, wasmtime Engine
//! configuration, host function registration, and resource limiting.
//!
//! ## What lives here
//!
//! - wasmtime Engine setup, Config, and compilation cache
//! - Pre-warmed instance pool with state injection on acquire
//! - Host function definitions and linker configuration
//! - ResourceLimiter implementation for per-instance caps
//! - WASI context setup and preopens directory management
//!
//! ## What does NOT live here
//!
//! - Agent loop logic (see jadepaw-agent)
//! - Core data types (see jadepaw-core)
//! - HTTP gateway transport (see jadepaw-gateway)
//! - Skill compilation (see jadepaw-skill)
