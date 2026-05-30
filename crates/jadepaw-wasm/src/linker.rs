//! Host function registration on `Linker<SessionState>`.
//!
//! Registers all host functions under the `"jadepaw"` namespace (D-02).
//! Uses `func_wrap_async` for async host functions — critical for Phase 3
//! LLM streaming support.
//!
//! # Design (D-02)
//!
//! - Well-known namespace: `"jadepaw"`
//! - `func_wrap_async` for all I/O host functions (not component model, per D-03)
//! - Each registration wraps a host function in `host/` module
//!
//! # Usage
//!
//! ```rust,ignore
//! let engine = EngineFactory::build()?;
//! let mut linker = create_linker(&engine)?;
//! register_host_functions(&mut linker)?;
//! ```

use wasmtime::{Engine, Linker};

use crate::host::{file_read_host_fn, file_write_host_fn, http_request_host_fn, log_message_host_fn};
use crate::session::SessionState;

/// Create a new `Linker<SessionState>` for the given Engine.
///
/// Returns an empty linker ready for host function registration.
/// The linker references the Engine's compilation cache.
pub fn create_linker(engine: &Engine) -> Linker<SessionState> {
    Linker::new(engine)
}

/// Register all host functions on the linker under the `"jadepaw"` namespace.
///
/// # Registered imports
///
/// | Import path | Function | Notes |
/// |-------------|----------|-------|
/// | `jadepaw.log_message` | `log_message_host_fn` | Always allowed (safe default) |
/// | `jadepaw.file_read` | `file_read_host_fn` | Capability-gated + path-validated |
/// | `jadepaw.file_write` | `file_write_host_fn` | Capability-gated + path-validated |
/// | `jadepaw.http_request` | `http_request_host_fn` | Capability-gated + domain-validated |
///
/// # Errors
///
/// Returns an error if a function name is already registered on the linker
/// (shadowing prevention).
pub fn register_host_functions(linker: &mut Linker<SessionState>) -> anyhow::Result<()> {
    // log_message: always allowed, no capability check
    linker
        .func_wrap_async(
            "jadepaw",
            "log_message",
            |caller, (level_ptr, level_len, msg_ptr, msg_len): (i32, i32, i32, i32)| {
                log_message_host_fn(caller, level_ptr, level_len, msg_ptr, msg_len)
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to register log_message: {e}"))?;

    // file_read: capability-gated + path-validated (D-11, SEC-03)
    linker
        .func_wrap_async(
            "jadepaw",
            "file_read",
            |caller, (path_ptr, path_len, buf_ptr, buf_len): (i32, i32, i32, i32)| {
                file_read_host_fn(caller, path_ptr, path_len, buf_ptr, buf_len)
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to register file_read: {e}"))?;

    // file_write: capability-gated + path-validated (D-11, SEC-03)
    linker
        .func_wrap_async(
            "jadepaw",
            "file_write",
            |caller, (path_ptr, path_len, data_ptr, data_len): (i32, i32, i32, i32)| {
                file_write_host_fn(caller, path_ptr, path_len, data_ptr, data_len)
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to register file_write: {e}"))?;

    // http_request: capability-gated + domain-validated (stub in Phase 2)
    linker
        .func_wrap_async(
            "jadepaw",
            "http_request",
            |caller,
             (method_ptr, method_len, url_ptr, url_len, headers_ptr, headers_len, body_ptr, body_len): (
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
                i32,
            )| {
                http_request_host_fn(
                    caller,
                    method_ptr,
                    method_len,
                    url_ptr,
                    url_len,
                    headers_ptr,
                    headers_len,
                    body_ptr,
                    body_len,
                )
            },
        )
        .map_err(|e| anyhow::anyhow!("failed to register http_request: {e}"))?;

    Ok(())
}