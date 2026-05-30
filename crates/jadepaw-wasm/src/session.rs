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

use crate::limits::instance_hard::InstanceHardLimiter;

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
}

impl SessionState {
    /// Create a new session state with the given identity and capabilities.
    ///
    /// The `SessionLimits` are initialized from `capabilities.max_memory_mb`.
    pub fn new(
        session_id: SessionId,
        tenant_id: TenantId,
        capabilities: InstanceCapabilities,
    ) -> Self {
        let hard_limit = InstanceHardLimiter::new(capabilities.max_memory_mb);
        Self {
            session_id,
            tenant_id,
            capabilities,
            limits: SessionLimits { hard_limit },
            created_at: chrono::Utc::now(),
        }
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