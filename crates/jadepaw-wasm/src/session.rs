//! SessionState — the per-session data stored in `Store<T>`.
//!
//! Each wasmtime Store holds a `SessionState` that carries session identity,
//! capability whitelist, and resource limits. Host functions access this data
//! via `caller.data()` and `caller.data_mut()`.
//!
//! # Design (D-04, D-11)
//!
//! - Store-per-session: `Store::new(engine, session_state)` per session, never reused
//! - ResourceLimiter registered via `store.limiter(|s| &mut s.limits)`
//! - Capability checks via `caller.data().capabilities`

use jadepaw_core::{InstanceCapabilities, SessionId, TenantId};
use std::fmt;
use std::path::PathBuf;

use crate::limits::instance_hard::InstanceHardLimiter;
use reqwest::redirect;
use std::time::Duration;

/// Resource limits for a single session.
///
/// Owns the per-instance hard limiter. Registered on the Store via
/// `store.limiter()` closure (Pitfall 4).
#[derive(Clone)]
pub struct SessionLimits {
    /// Per-instance hard memory cap (64MB by default, from capabilities).
    pub hard_limit: InstanceHardLimiter,
}

impl fmt::Debug for SessionLimits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionLimits")
            .field("hard_limit", &"InstanceHardLimiter { ... }")
            .finish()
    }
}

/// Per-session state stored in `Store<T>`.
///
/// Created fresh for each session, dropped when the session ends.
/// Contains session identity, capability whitelist, and resource limits.
#[derive(Clone)]
pub struct SessionState {
    /// Unique session identifier.
    pub session_id: SessionId,
    /// Tenant that owns this session.
    pub tenant_id: TenantId,
    /// Capability whitelist declared at instance initialization.
    pub capabilities: InstanceCapabilities,
    /// Resource limiters registered on the Store.
    pub limits: SessionLimits,
    /// Session creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Sandbox root directory for path containment (D-11).
    ///
    /// All guest-provided file paths are validated against this root via
    /// `validate_sandbox_path` before any I/O is performed.
    pub sandbox_root: PathBuf,
    /// Shared HTTP client for Wasm guest host function calls.
    ///
    /// Reusing a single client across all `http_request` host function
    /// invocations avoids the resource leak of constructing a fresh
    /// reqwest::Client (with connection pool, TLS session cache, DNS resolver)
    /// on every call. Initialized once during session creation.
    pub http_client: reqwest::Client,
}

impl SessionState {
    /// Create a new session state with the given identity, capabilities,
    /// and sandbox root directory.
    ///
    /// The `SessionLimits` are initialized from `capabilities.max_memory_mb`.
    ///
    /// `sandbox_root` is used by `validate_sandbox_path` to enforce path
    /// containment for all filesystem host functions.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client (reqwest) fails to initialize,
    /// e.g., due to missing TLS support.
    pub fn new(
        session_id: SessionId,
        tenant_id: TenantId,
        capabilities: InstanceCapabilities,
        sandbox_root: PathBuf,
    ) -> anyhow::Result<Self> {
        let hard_limit = InstanceHardLimiter::new(capabilities.max_memory_mb);
        let http_client = reqwest::Client::builder()
            .redirect(redirect::Policy::limited(1))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build reqwest client for session: {e}"))?;
        Ok(Self {
            session_id,
            tenant_id,
            capabilities,
            limits: SessionLimits { hard_limit },
            created_at: chrono::Utc::now(),
            sandbox_root,
            http_client,
        })
    }

    /// Create a session state with default (empty) capabilities and a given
    /// sandbox root. Convenience for testing.
    ///
    /// # Panics
    ///
    /// Panics if the reqwest HTTP client fails to initialize (e.g., no TLS support).
    /// This is acceptable for tests — production code should use `new()` and propagate
    /// the error.
    pub fn with_defaults(sandbox_root: PathBuf) -> Self {
        Self::new(
            SessionId::new(),
            TenantId::new(),
            InstanceCapabilities::default(),
            sandbox_root,
        )
        .expect("HTTP client initialization required for tests")
    }
}

impl fmt::Debug for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionState")
            .field("session_id", &self.session_id)
            .field("tenant_id", &self.tenant_id)
            .field("capabilities", &self.capabilities)
            .field("limits", &self.limits)
            .field("created_at", &self.created_at)
            .finish()
    }
}