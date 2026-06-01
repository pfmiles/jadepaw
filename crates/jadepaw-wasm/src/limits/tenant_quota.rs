//! TenantQuotaLimiter — tenant-level aggregate resource budget.
//!
//! Wraps an `InstanceHardLimiter` and enforces tenant-level aggregate memory
//! budgets. Returns `Ok(false)` when the tenant budget is exceeded (guest
//! receives -1 from `memory.grow`, recoverable), then delegates to the inner
//! limiter for per-instance hard cap checks.
//!
//! Note: `TenantQuotaLimiter` is implemented and tested but not yet wired
//! into the `InstancePool`/`SessionState` infrastructure. It will be activated
//! when per-tenant aggregate memory tracking is needed (Phase 4).
//!
//! # Design (D-07, D-08, D-09a)
//!
//! - `Ok(true)`: growth is within tenant budget AND instance hard cap
//! - `Ok(false)`: tenant budget exceeded (recoverable — guest gets -1)
//! - `Err()`: inner InstanceHardLimiter hard cap exceeded (trap)
//!
//! # Extensibility (D-09a)
//!
//! The delegating chain architecture allows adding `ToolRateLimiter` (Phase 4),
//! `SessionMemoryLimiter` (Phase 5), or `DistributedTenantQuotaLimiter`
//! without touching the security-critical `InstanceHardLimiter`.

use crate::limits::instance_hard::InstanceHardLimiter;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use wasmtime::ResourceLimiter;

/// Tenant-level aggregate memory budget limiter.
///
/// Tracks total memory allocated across all instances belonging to a tenant
/// via an `Arc<AtomicUsize>`. The budget counter is shared across all
/// stores of the same tenant.
#[derive(Clone)]
pub struct TenantQuotaLimiter {
    tenant_budget_used: Arc<AtomicUsize>,
    tenant_budget_max: usize,
    inner: InstanceHardLimiter,
}

impl TenantQuotaLimiter {
    /// Create a new tenant quota limiter with the given aggregate budget in
    /// megabytes, wrapping the provided inner hard limiter.
    ///
    /// The `tenant_budget_used` counter is shared across all instances
    /// belonging to the same tenant.
    pub fn new(
        budget_max_mb: u32,
        tenant_budget_used: Arc<AtomicUsize>,
        inner: InstanceHardLimiter,
    ) -> Self {
        Self {
            tenant_budget_used,
            tenant_budget_max: (budget_max_mb as usize) * 1024 * 1024,
            inner,
        }
    }

    /// Convenience alias for `new()`. Used primarily in tests to clarify intent
    /// when constructing limiters with small byte-scale budgets.
    #[doc(hidden)]
    pub fn new_with_budget(
        budget_max_mb: u32,
        tenant_budget_used: Arc<AtomicUsize>,
        inner: InstanceHardLimiter,
    ) -> Self {
        Self::new(budget_max_mb, tenant_budget_used, inner)
    }
}

impl ResourceLimiter for TenantQuotaLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let delta = desired.saturating_sub(current);

        // Check tenant aggregate budget using Relaxed ordering — exact
        // fairness is not required; approximate tracking is sufficient.
        let used = self.tenant_budget_used.load(Ordering::Relaxed);
        if used + delta > self.tenant_budget_max {
            return Ok(false); // Recoverable: guest receives -1 from memory.grow
        }

        // Delegate to inner InstanceHardLimiter for the per-instance hard cap FIRST.
        // Only commit the tenant budget if inner approves — prevents counter leak
        // when the inner limiter rejects the growth (CR-01).
        let inner_result = self.inner.memory_growing(current, desired, maximum)?;

        // Now it's safe to commit the tenant budget:
        self.tenant_budget_used
            .fetch_add(delta, Ordering::Relaxed);
        Ok(inner_result)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        // Tables are tracked through the same aggregate budget.
        let delta = desired.saturating_sub(current);
        let used = self.tenant_budget_used.load(Ordering::Relaxed);
        if used + delta > self.tenant_budget_max {
            return Ok(false);
        }
        // Delegate first, commit budget only on success (CR-02)
        let inner_result = self.inner.table_growing(current, desired, maximum)?;
        self.tenant_budget_used
            .fetch_add(delta, Ordering::Relaxed);
        Ok(inner_result)
    }
}

impl std::fmt::Debug for TenantQuotaLimiter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantQuotaLimiter")
            .field("tenant_budget_max", &self.tenant_budget_max)
            .field("inner", &"InstanceHardLimiter { ... }")
            .finish()
    }
}