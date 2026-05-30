//! InstancePool — lazy-instantiation pool with concurrency bounding.
//!
//! Manages a shared pool of pre-compiled wasmtime `InstancePre` objects.
//! Each `acquire()` creates a fresh `Store` with session-specific state,
//! enforces concurrency limits via `tokio::sync::Semaphore`, and tracks
//! active sessions in a `DashMap<SessionId, SessionHandle>`.
//!
//! # Design (D-04, D-05)
//!
//! - **Lazy instantiation**: Pre-compiled `Module` + `InstancePre` (shared via `Arc`).
//!   `acquire()` = `Store::new()` + `instance_pre.instantiate_async()`.
//! - **Concurrency bound**: `tokio::sync::Semaphore` limits max concurrent sessions.
//!   `acquire()` blocks (async) when pool is at capacity.
//! - **Session tracking**: `DashMap<SessionId, SessionHandle>` provides O(1) lookup
//!   of active sessions per D-05.
//!
//! # Pitfall Prevention
//!
//! - **Pitfall 2**: Store-per-session, never reused. Each `acquire()` calls `Store::new()`.
//! - **Pitfall 7**: Store dropped on `SessionHandle::drop()` — wasmtime PoolingAllocator
//!   zeros memory slots before reuse (verified by isolation test).
//! - **Pitfall 4**: `PoolingAllocationConfig::max_memory_size(64MB)` set in `EngineFactory`.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use wasmtime::{Engine, Instance, InstancePre, Linker, Module, Store};

use crate::engine::EngineFactory;
use crate::linker::{create_linker, register_host_functions};
use crate::session::SessionState;
use jadepaw_core::SessionId;

/// Configuration for creating an `InstancePool`.
///
/// `guest_bytes` is the compiled or WAT guest module binary. `sandbox_root`
/// is the base directory for path containment. `max_concurrent` bounds the
/// number of simultaneously active sessions.
#[derive(Clone)]
pub struct PoolConfig {
    /// Compiled guest module binary (Wasm bytes).
    pub guest_bytes: Vec<u8>,
    /// Base directory for path containment (used by host functions).
    pub sandbox_root: PathBuf,
    /// Maximum number of concurrent sessions.
    pub max_concurrent: usize,
}

impl PoolConfig {
    /// Create a new pool configuration.
    pub fn new(guest_bytes: Vec<u8>, sandbox_root: PathBuf, max_concurrent: usize) -> Self {
        Self {
            guest_bytes,
            sandbox_root,
            max_concurrent,
        }
    }
}

/// A handle to an active session acquired from the pool.
///
/// Contains the per-session `Store`, the instantiated guest `Instance`,
/// and the semaphore permit that holds the concurrency slot.
///
/// Dropping a `SessionHandle`:
/// - Removes the session from the `active_sessions` DashMap
/// - Drops the `Store`, `Instance`, and `OwnedSemaphorePermit`
/// - The PoolingAllocator reclaims the memory slot (Pitfall 7 — zeroed before reuse)
pub struct SessionHandle {
    /// Per-session store with SessionState data.
    store: Store<SessionState>,
    /// The instantiated guest module.
    instance: Instance,
    /// Owned semaphore permit — released on drop.
    _permit: OwnedSemaphorePermit,
    /// The session identifier.
    session_id: SessionId,
    /// Reference to the pool's active sessions map for cleanup on drop.
    active_sessions: Arc<DashMap<SessionId, ()>>,
}

impl SessionHandle {
    /// Returns the session identifier.
    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    /// Returns a reference to the instantiated guest `Instance`.
    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    /// Returns a reference to the per-session `Store`.
    pub fn store(&self) -> &Store<SessionState> {
        &self.store
    }

    /// Returns a mutable reference to the per-session `Store`.
    pub fn store_mut(&mut self) -> &mut Store<SessionState> {
        &mut self.store
    }
}

impl Drop for SessionHandle {
    fn drop(&mut self) {
        // Remove from active sessions tracking (D-05).
        // The store, instance, and permit are dropped automatically.
        self.active_sessions.remove(&self.session_id);
    }
}

/// The instance pool — manages lazy instantiation of guest Wasm sessions.
///
/// Created once per guest module (Skill). Shared across all sessions for
/// that guest via `Arc`-wrapped `InstancePre`.
///
/// # Thread safety
///
/// `InstancePool` is `Send + Sync`. Multiple concurrent callers can call
/// `acquire()` simultaneously — the `Semaphore` bounds concurrency.
pub struct InstancePool {
    /// Shared wasmtime Engine (one per process).
    engine: Engine,
    /// Pre-compiled linker configuration for the guest module.
    _linker: Linker<SessionState>,
    /// Pre-compiled instance template (shared across all sessions).
    instance_pre: Arc<InstancePre<SessionState>>,
    /// Concurrency bound — max concurrent sessions.
    semaphore: Arc<Semaphore>,
    /// Active session tracking (D-05).
    active_sessions: Arc<DashMap<SessionId, ()>>,
}

impl InstancePool {
    /// Create a new instance pool from the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `max_concurrent` is 0
    /// - Engine creation fails
    /// - Guest module compilation fails
    /// - Linker configuration or host function registration fails
    /// - `InstancePre` instantiation fails
    pub fn new(config: PoolConfig) -> anyhow::Result<Self> {
        if config.max_concurrent == 0 {
            return Err(anyhow::anyhow!(
                "PoolConfig::max_concurrent must be > 0"
            ));
        }

        let engine = EngineFactory::build()?;
        let module = Module::new(&engine, &config.guest_bytes)
            .map_err(|e| anyhow::anyhow!("failed to compile guest module: {e}"))?;
        let mut linker = create_linker(&engine);
        register_host_functions(&mut linker)?;
        let instance_pre =
            Arc::new(
                linker
                    .instantiate_pre(&module)
                    .map_err(|e| anyhow::anyhow!("failed to create InstancePre: {e}"))?,
            );

        Ok(Self {
            engine,
            _linker: linker,
            instance_pre,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
            active_sessions: Arc::new(DashMap::new()),
        })
    }

    /// Acquire a session from the pool.
    ///
    /// Blocks (asynchronously) if the pool is at capacity. When a slot becomes
    /// available, creates a fresh `Store` with the provided `state`, configures
    /// fuel/epoch/limiters, and instantiates the guest module.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `InstancePre::instantiate_async` fails
    /// - Fuel/epoch configuration fails
    pub async fn acquire(
        &self,
        session_id: SessionId,
        state: SessionState,
    ) -> anyhow::Result<SessionHandle> {
        // Acquire semaphore permit (D-05: blocks when at capacity)
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("semaphore closed — pool is shutting down"))?;

        // Create fresh Store per session (D-04, Pitfall 2)
        let mut store = Store::new(&self.engine, state);

        // Configure fuel metering (Pitfall 1)
        store.set_fuel(1_000_000).map_err(|e| {
            anyhow::anyhow!("failed to set fuel on store: {e}")
        })?;

        // Configure epoch interruption (Pitfall 1)
        store.epoch_deadline_async_yield_and_update(100);

        // Configure resource limiter (D-07, Pitfall 4)
        store.limiter(|s| &mut s.limits.hard_limit);

        // Instantiate guest module in this Store (D-04 lazy instantiation)
        let instance = self
            .instance_pre
            .instantiate_async(&mut store)
            .await
            .map_err(|e| anyhow::anyhow!("failed to instantiate guest module: {e}"))?;

        let handle = SessionHandle {
            store,
            instance,
            _permit: permit,
            session_id,
            active_sessions: Arc::clone(&self.active_sessions),
        };

        // Track active session (D-05)
        self.active_sessions.insert(session_id, ());

        Ok(handle)
    }

    /// Return the number of currently active sessions.
    pub fn active_count(&self) -> usize {
        self.active_sessions.len()
    }

    /// Return the pool's maximum capacity.
    pub fn capacity(&self) -> usize {
        self.semaphore.available_permits() + self.active_count()
    }
}