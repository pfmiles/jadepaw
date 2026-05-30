//! InstanceHardLimiter — per-instance security boundary.
//!
//! Enforces a hard per-instance memory cap at 64MB (configurable).
//! Returns `Err()` (trap, Store poisoned) when the limit is exceeded.
//! This is the innermost limiter in the delegating chain — it must be
//! the last check before wasmtime approves memory growth.
//!
//! # Design (D-07, D-08)
//!
//! - `Ok(true)`: growth is within the instance hard cap, proceed
//! - `Err()`: hard cap exceeded, Store is terminally poisoned (security boundary)
//! - The 64MB cap MUST match `PoolingAllocationConfig::max_memory_size` (Pitfall 4)

use wasmtime::ResourceLimiter;

/// Hard per-instance memory limit. Returns `Err()` when exceeded.
///
/// This is a security boundary — any violation results in a trap and
/// the Store becoming poisoned.
#[derive(Debug, Clone)]
pub struct InstanceHardLimiter {
    max_bytes: usize,
}

impl InstanceHardLimiter {
    /// Create a new hard limiter with the given memory cap in megabytes.
    ///
    /// # Panics
    ///
    /// Panics if `max_mb` is 0 in debug builds (no-instance memory is useless);
    /// allows 0 in release as a deliberate lock-down option.
    pub fn new(max_mb: u32) -> Self {
        debug_assert!(max_mb > 0, "InstanceHardLimiter max_mb should be > 0");
        Self {
            max_bytes: (max_mb as usize) * 1024 * 1024,
        }
    }
}

impl ResourceLimiter for InstanceHardLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.max_bytes {
            return Err(wasmtime::Error::new(
                std::io::Error::new(
                    std::io::ErrorKind::OutOfMemory,
                    format!(
                        "instance memory limit exceeded: {} bytes requested, {} bytes max",
                        desired, self.max_bytes
                    ),
                ),
            ));
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Tables are not restricted by the instance hard cap — we allow
        // table growth unconditionally. Table exhaustion is gated by the
        // pooling allocator's table limit, not by the instance cap.
        Ok(true)
    }
}