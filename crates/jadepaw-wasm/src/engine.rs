//! EngineFactory — builds a safety-configured wasmtime Engine.
//!
//! Creates a wasmtime `Engine` with all safety features enabled from Day 1:
//! Fuel metering, Epoch interruption, PoolingAllocator with 64MB slots,
//! Cranelift JIT compilation, and async support.
//!
//! # Safety configuration (D-09)
//!
//! - `consume_fuel(true)` — instruction-count-based execution metering (Pitfall 1)
//! - `epoch_interruption(true)` — epoch-based cooperative yielding (Pitfall 1)
//! - `PoolingAllocationConfig` with `max_memory_size(64MB)` — pre-allocated
//!   memory slots for 10k+ concurrent instances (Pitfall 2)
//! - `async_support(true)` — async Store operations for Phase 3 LLM streaming
//! - Cranelift `OptLevel::Speed` — optimized JIT compilation
//!
//! # Critical constraint (Pitfall 4)
//!
//! `PoolingAllocationConfig::max_memory_size(64 * 1024 * 1024)` MUST match the
//! `InstanceHardLimiter` 64MB per-instance cap. Mismatch causes either:
//! - Slots larger than 64MB: virtual address space exhaustion, fewer concurrent
//!   instances than expected
//! - Slots smaller than 64MB: instances denied growth at the pool level even
//!   when within the per-instance cap

use wasmtime::{Config, Engine, InstanceAllocationStrategy, OptLevel, PoolingAllocationConfig};

/// Factory for building a safety-configured wasmtime Engine.
///
/// The Engine is the expensive resource — created once per process lifetime
/// and shared across all sessions. Configuration is immutable after creation.
pub struct EngineFactory;

impl EngineFactory {
    /// Build a wasmtime Engine with all safety features enabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the Engine cannot be created with the given
    /// configuration. This is typically due to insufficient system resources
    /// (e.g., not enough virtual address space for PoolingAllocationConfig slots).
    pub fn build() -> anyhow::Result<Engine> {
        let mut config = Config::default();

        // Safety: both Fuel AND Epoch from Day 1 (Pitfall 1 prevention)
        config.consume_fuel(true);
        config.epoch_interruption(true);

        // Performance: Cranelift JIT with speed optimizations
        config.cranelift_opt_level(OptLevel::Speed);

        // PoolingAllocator: pre-allocate memory slots for 10k+ concurrent instances
        let mut pooling = PoolingAllocationConfig::default();
        // CRITICAL: MUST match InstanceHardLimiter 64MB cap (Pitfall 4)
        pooling.max_memory_size(64 * 1024 * 1024);
        // Keep warm slots for sub-ms instantiation latency
        pooling.max_unused_warm_slots(100);

        config.allocation_strategy(InstanceAllocationStrategy::Pooling(pooling));

        Engine::new(&config).map_err(|e| anyhow::anyhow!("failed to create wasmtime Engine: {}", e))
    }
}