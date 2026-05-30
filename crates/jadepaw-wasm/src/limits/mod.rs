//! ResourceLimiter implementations for per-instance and per-tenant boundaries.
//!
//! The limiter chain follows the delegating pattern (D-07). Each limiter is
//! independently testable and new limiters can be prepended without touching
//! existing ones (D-09a).

pub mod instance_hard;
pub mod tenant_quota;

pub use instance_hard::InstanceHardLimiter;
pub use tenant_quota::TenantQuotaLimiter;