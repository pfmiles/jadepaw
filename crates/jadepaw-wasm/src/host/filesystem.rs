//! `file_read` and `file_write` host function implementations.
//!
//! # Defense sequence (D-11, D-12, SEC-03)
//!
//! For every filesystem operation:
//! 1. Access `caller.data()` at entry (D-11)
//! 2. Bounds-check guest memory (ptr, len) against `memory.data_size` (Pitfall 3)
//! 3. Capability check: `can_read_file()` / `can_write_file()` (D-11, T-02-07)
//! 4. Path validation: `validate_sandbox_path(guest_path, sandbox_root)` (SEC-03, T-02-08)
//! 5. Only then: perform I/O
//!
//! If any check fails, the operation is rejected BEFORE any side effects.

use std::future::Future;

use tracing::warn;
use wasmtime::Caller;

use crate::path::validate_sandbox_path;
use crate::session::SessionState;

/// Host function for `jadepaw.file_read`.
///
/// # Signature (guest import)
///
/// `(path_ptr: i32, path_len: i32, buf_ptr: i32, buf_len: i32) -> i32`
///
/// - `path_ptr`/`path_len`: file path to read
/// - `buf_ptr`/`buf_len`: buffer in guest memory to write file contents
/// - Returns: number of bytes read on success, -1 on error
///
/// # Threat mitigations
///
/// - T-02-07 (Elevation of Privilege): `can_read_file()` checked before every read
/// - T-02-08 (Path Tampering): `validate_sandbox_path` rejects traversal
/// - T-02-09 (Info Disclosure): guest memory bounds-checked
pub fn file_read_host_fn(
    mut caller: Caller<'_, SessionState>,
    path_ptr: i32,
    path_len: i32,
    buf_ptr: i32,
    buf_len: i32,
) -> Box<dyn Future<Output = i32> + Send + '_> {
    Box::new(async move {
        // Step 1: Access SessionState (D-11)
        let (session_id, sandbox_root) = {
            let state = caller.data();
            (state.session_id, state.sandbox_root.clone())
        };

        // Get guest memory handle (held for bounds checking; clone is cheap)
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(mem) => mem,
            None => {
                warn!(%session_id, "file_read: no exported memory in guest module");
                return -1;
            }
        };

        let mem_data = memory.data(&caller);
        let mem_size = memory.data_size(&caller);

        // Step 2: Bounds-check path pointer (WR-01: use checked_add to prevent overflow)
        let path_start = path_ptr as usize;
        let path_len_usize = path_len as usize;
        let path_end = path_start.saturating_add(path_len_usize);
        if path_end > mem_size {
            warn!(%session_id, "file_read: path pointer out of bounds (start={}, len={})", path_ptr, path_len);
            return -1;
        }
        let path = match std::str::from_utf8(&mem_data[path_start..path_end]) {
            Ok(s) => s,
            Err(e) => {
                warn!(%session_id, "file_read: invalid UTF-8 in path: {}", e);
                return -1;
            }
        };

        // Step 3: Capability check (T-02-07, default deny D-12)
        {
            let can_read = caller.data().can_read_file(path);
            if !can_read {
                warn!(%session_id, "file_read: CapabilityDenied for path '{}'", path);
                return -1;
            }
        }

        // Step 4: Path validation (SEC-03, T-02-08)
        let safe_path = match validate_sandbox_path(path, &sandbox_root) {
            Ok(p) => p,
            Err(e) => {
                warn!(%session_id, "file_read: path validation failed: {} (path='{}')", e, path);
                return -1;
            }
        };

        // Step 5: Perform I/O only after all checks pass
        let contents = match tokio::fs::read(&safe_path).await {
            Ok(c) => c,
            Err(e) => {
                warn!(%session_id, "file_read: I/O error reading '{}': {}", safe_path.display(), e);
                return -1;
            }
        };

        // Sanitize guest-provided buf_len: negative values saturate to 0
        // (will be rejected as "buffer too small" below instead of panicking)
        let buf_len_usize = if buf_len > 0 { buf_len as usize } else { 0 };

        // Write result to guest memory via memory.write (bounds-checked)
        let n = contents.len() as i32;
        if n as usize <= buf_len_usize {
            // buf_ptr is guest-controlled and untrusted — memory.write can still fail
            // if buf_ptr + contents.len() exceeds actual memory bounds.
            match memory.write(&mut caller, buf_ptr as usize, &contents) {
                Ok(()) => n,
                Err(e) => {
                    warn!(%session_id, "file_read: memory.write failed (buf_ptr={}, len={}): {}", buf_ptr, n, e);
                    -1
                }
            }
        } else {
            warn!(%session_id, "file_read: output buffer too small (need {}, have {})", n, buf_len);
            -1 // buffer too small — no partial write to avoid ambiguous state
        }
    })
}

/// Host function for `jadepaw.file_write`.
///
/// # Signature (guest import)
///
/// `(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32) -> i32`
///
/// - `path_ptr`/`path_len`: file path to write
/// - `data_ptr`/`data_len`: data to write from guest memory
/// - Returns: 0 on success, -1 on error
///
/// The file is created if it does not exist, truncated if it does.
///
/// # Threat mitigations
///
/// - T-02-07 (Elevation of Privilege): `can_write_file()` checked before every write
/// - T-02-08 (Path Tampering): `validate_sandbox_path` rejects traversal
/// - T-02-09 (Info Disclosure): guest memory bounds-checked
pub fn file_write_host_fn(
    mut caller: Caller<'_, SessionState>,
    path_ptr: i32,
    path_len: i32,
    data_ptr: i32,
    data_len: i32,
) -> Box<dyn Future<Output = i32> + Send + '_> {
    Box::new(async move {
        // Step 1: Access SessionState (D-11)
        let (session_id, sandbox_root) = {
            let state = caller.data();
            (state.session_id, state.sandbox_root.clone())
        };

        // Get guest memory
        let memory = match caller.get_export("memory").and_then(|e| e.into_memory()) {
            Some(mem) => mem,
            None => {
                warn!(%session_id, "file_write: no exported memory in guest module");
                return -1;
            }
        };

        let mem_data = memory.data(&caller);
        let mem_size = memory.data_size(&caller);

        // Step 2: Bounds-check path pointer (WR-01: use checked_add to prevent overflow)
        let path_start = path_ptr as usize;
        let path_len_usize = path_len as usize;
        let path_end = path_start.saturating_add(path_len_usize);
        if path_end > mem_size {
            warn!(%session_id, "file_write: path pointer out of bounds");
            return -1;
        }
        let path = match std::str::from_utf8(&mem_data[path_start..path_end]) {
            Ok(s) => s,
            Err(e) => {
                warn!(%session_id, "file_write: invalid UTF-8 in path: {}", e);
                return -1;
            }
        };

        // Bounds-check data pointer (WR-01: use checked_add to prevent overflow)
        let data_start = data_ptr as usize;
        let data_len_usize = data_len as usize;
        let data_end = data_start.saturating_add(data_len_usize);
        if data_end > mem_size {
            warn!(%session_id, "file_write: data pointer out of bounds");
            return -1;
        }

        // Step 3: Capability check (T-02-07, default deny D-12)
        {
            let can_write = caller.data().can_write_file(path);
            if !can_write {
                warn!(%session_id, "file_write: CapabilityDenied for path '{}'", path);
                return -1;
            }
        }

        // Copy data out of guest memory before validation (bounds-checked above)
        let data = mem_data[data_start..data_end].to_vec();

        // Step 4: Path validation (SEC-03, T-02-08)
        let safe_path = match validate_sandbox_path(path, &sandbox_root) {
            Ok(p) => p,
            Err(e) => {
                warn!(%session_id, "file_write: path validation failed: {} (path='{}')", e, path);
                return -1;
            }
        };

        // Step 5: Perform I/O only after all checks pass
        match tokio::fs::write(&safe_path, &data).await {
            Ok(()) => 0,
            Err(e) => {
                warn!(%session_id, "file_write: I/O error writing '{}': {}", safe_path.display(), e);
                -1
            }
        }
    })
}