//! Canonical guest-host communication contract.
//!
//! The `HostFunctions` trait catalogues every host function import that a
//! guest Wasm module can call. This trait is the single source of truth for
//! the guest-host interface and lives in `jadepaw-core` so that downstream
//! crates (jadepaw-agent, jadepaw-skill) can reference it without depending
//! on `jadepaw-wasm`.
//!
//! # Design constraints (D-01)
//!
//! - **Additive-only**: methods may be added, never removed. Breaking changes
//!   require a major version bump of the trait.
//! - **CI-verifiable**: CI must verify that every implementor of this trait
//!   covers all methods. A missing method is a compile error.
//! - **Zero dep**: the trait signature uses only std types and types from
//!   this crate (JadepawError). No wasmtime dependency.
//!
//! # Migration path (D-03)
//!
//! When (if) the WIT Component Model reaches Phase 3-4 maturity, this trait
//! can be mapped onto WIT as a compatibility shim. The function signatures
//! are designed to be mappable to WIT imports.

use crate::error::Result;
use async_trait::async_trait;

/// The canonical, versioned interface contract for guest-host communication.
///
/// Every host function import that a guest Wasm module can call MUST be
/// represented as a method on this trait. Implementors register these
/// methods with wasmtime's `Linker` in `jadepaw-wasm`.
///
/// # Additive-only policy
///
/// Methods may be added, never removed. CI must verify all implementors
/// cover every method.
#[async_trait]
pub trait HostFunctions: Send + Sync {
    /// Log a message at the given level.
    ///
    /// The host decides how to route log messages (tracing, file, stdout).
    /// Guests should use this for diagnostics, not for large data output.
    async fn log_message(&self, level: String, message: String) -> Result<()>;

    /// Read the contents of a file at the given path.
    ///
    /// The path is validated against the session's sandbox root before
    /// any I/O is performed. The capability whitelist (`can_read_files`) is
    /// checked first.
    async fn file_read(&self, path: String) -> Result<Vec<u8>>;

    /// Write data to a file at the given path.
    ///
    /// The path is validated against the session's sandbox root before
    /// any I/O is performed. The capability whitelist (`can_write_files`) is
    /// checked first. The file is created if it does not exist, truncated
    /// if it does.
    async fn file_write(&self, path: String, data: Vec<u8>) -> Result<()>;
}