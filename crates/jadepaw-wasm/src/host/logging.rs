//! `log_message` host function — safe default (no capability check).
//!
//! Logging is always allowed (no capability gating). The host routes log
//! messages via `tracing`, always including the `session_id` from
//! `caller.data()` for auditability (T-02-12).

use std::future::Future;

use tracing::{error, info, warn};
use wasmtime::Caller;

use crate::session::SessionState;

/// Host function for `jadepaw.log_message`.
///
/// # Signature (guest import)
///
/// `(level_ptr: i32, level_len: i32, msg_ptr: i32, msg_len: i32) -> i32`
///
/// - `level_ptr`/`level_len`: string like "info", "warn", "error"
/// - `msg_ptr`/`msg_len`: log message string
/// - Returns 0 on success
///
/// # Safety
///
/// - Guest memory (ptr, len) pairs are bounds-checked against `memory.data_size`
/// - Level and message are read as UTF-8 strings; invalid UTF-8 is caught
/// - No capability check — logging is always allowed (safe default, D-11)
/// - `session_id` is always included in the log for auditability (T-02-12)
pub fn log_message_host_fn(
    mut caller: Caller<'_, SessionState>,
    level_ptr: i32,
    level_len: i32,
    msg_ptr: i32,
    msg_len: i32,
) -> Box<dyn Future<Output = i32> + Send + '_> {
    Box::new(async move {
        // Access SessionState at entry (D-11)
        let state = caller.data();
        let session_id = state.session_id;

        // Get guest memory with bounds checking (Pitfall 3, T-02-09)
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(mem) => mem,
            None => {
                tracing::error!("log_message: no exported memory in guest module");
                return -1;
            }
        };

        let mem_data = memory.data(&caller);
        let mem_size = memory.data_size(&caller);

        // Bounds-check level string (WR-01: use checked_add to prevent overflow)
        let level_start = level_ptr as usize;
        let level_len_usize = level_len as usize;
        let level_end = level_start.checked_add(level_len_usize).unwrap_or(usize::MAX);
        if level_end > mem_size {
            tracing::warn!(
                %session_id,
                "log_message: level pointer out of bounds (start={}, len={}, mem_size={})",
                level_ptr, level_len, mem_size
            );
            return -1;
        }
        let level = match std::str::from_utf8(&mem_data[level_start..level_end]) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%session_id, "log_message: invalid UTF-8 in level: {}", e);
                return -1;
            }
        };

        // Bounds-check message string (WR-01: use checked_add to prevent overflow)
        let msg_start = msg_ptr as usize;
        let msg_len_usize = msg_len as usize;
        let msg_end = msg_start.checked_add(msg_len_usize).unwrap_or(usize::MAX);
        if msg_end > mem_size {
            tracing::warn!(
                %session_id,
                "log_message: message pointer out of bounds (start={}, len={}, mem_size={})",
                msg_ptr, msg_len, mem_size
            );
            return -1;
        }
        let message = match std::str::from_utf8(&mem_data[msg_start..msg_end]) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(%session_id, "log_message: invalid UTF-8 in message: {}", e);
                return -1;
            }
        };

        // Route to tracing based on level
        match level {
            "error" => error!(%session_id, "guest: {}", message),
            "warn" => warn!(%session_id, "guest: {}", message),
            _ => info!(%session_id, "guest: {}", message),
        }

        0 // success
    })
}