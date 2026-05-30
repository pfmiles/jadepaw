//! Integration tests for the InstancePool — acquire/release lifecycle,
//! session isolation, concurrency bounding, and benchmark latency.
//!
//! All tests use `#[tokio::test(flavor = "multi_thread")]` per CLAUDE.md
//! convention for wasmtime async tests.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use jadepaw_core::{InstanceCapabilities, SessionId, TenantId};
use jadepaw_wasm::{InstancePool, PoolConfig, SessionState};

/// Fixture: creates a SessionState with default capabilities and a temp sandbox root.
fn make_session_state() -> SessionState {
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();
    let capabilities = InstanceCapabilities::default();
    let sandbox_root = std::env::temp_dir().join(format!("jadepaw-pool-test-{}", uuid::Uuid::new_v4()));
    SessionState::new(session_id, tenant_id, capabilities, sandbox_root)
}

/// Fixture: creates noop.wasm guest bytes (module with empty _start).
fn noop_wasm_bytes() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "_start")))"#,
    )
    .expect("failed to parse noop.wat")
}

/// Creates a fresh InstancePool with noop.wasm guest and max 10 concurrent.
fn make_pool() -> InstancePool {
    let config = PoolConfig::new(
        noop_wasm_bytes(),
        PathBuf::from("/tmp/jadepaw-test"),
        10,
    );
    InstancePool::new(config).expect("failed to create pool")
}

// ── Test: pool creation ───────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_pool_create() {
    let pool = make_pool();
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.capacity(), 10);
}

// ── Test: acquire and release lifecycle ───────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_acquire_release() {
    let pool = make_pool();
    assert_eq!(pool.active_count(), 0);

    let session_id = SessionId::new();
    let state = make_session_state();
    let handle = pool
        .acquire(session_id, state)
        .await
        .expect("acquire failed");
    assert_eq!(pool.active_count(), 1);
    assert_eq!(handle.session_id(), &session_id);

    // Verify we can access the instance
    let _instance = handle.instance();

    drop(handle);
    assert_eq!(pool.active_count(), 0);
}

// ── Test: DashMap session tracking ────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_dashmap_session_tracking() {
    let pool = make_pool();
    let sid_a = SessionId::new();
    let sid_b = SessionId::new();

    let handle_a = pool
        .acquire(sid_a, make_session_state())
        .await
        .expect("acquire A failed");
    assert_eq!(pool.active_count(), 1);

    let handle_b = pool
        .acquire(sid_b, make_session_state())
        .await
        .expect("acquire B failed");
    assert_eq!(pool.active_count(), 2);

    drop(handle_a);
    assert_eq!(pool.active_count(), 1);

    drop(handle_b);
    assert_eq!(pool.active_count(), 0);
}

// ── Test: session isolation (SEC-01) ──────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_session_isolation() {
    let pool = make_pool();

    // Acquire session A, verify it has a different session_id than B
    let sid_a = SessionId::new();
    let state_a = make_session_state();
    let session_a_clone = state_a.session_id;
    let handle_a = pool
        .acquire(sid_a, state_a)
        .await
        .expect("acquire A failed");

    // Verify session A's store data has the right session_id
    {
        let session_id_in_store = handle_a.store().data().session_id;
        assert_eq!(session_id_in_store, session_a_clone);
    }

    // Release session A
    drop(handle_a);
    assert_eq!(pool.active_count(), 0);

    // Acquire session B — must have a different session_id
    let sid_b = SessionId::new();
    let state_b = make_session_state();
    let session_b_clone = state_b.session_id;
    let handle_b = pool
        .acquire(sid_b, state_b)
        .await
        .expect("acquire B failed");

    // Session B's store data must have its own session_id, not session A's
    {
        let session_id_in_store = handle_b.store().data().session_id;
        assert_eq!(session_id_in_store, session_b_clone);
        assert_ne!(session_id_in_store, session_a_clone);
    }

    drop(handle_b);
}

// ── Test: concurrency bound (D-05) ────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_concurrency_bound_blocks() {
    // Pool with capacity 1
    let config = PoolConfig::new(noop_wasm_bytes(), PathBuf::from("/tmp/jadepaw-test"), 1);
    let pool = Arc::new(InstancePool::new(config).expect("failed to create pool"));

    // Acquire the only slot
    let handle = pool
        .acquire(SessionId::new(), make_session_state())
        .await
        .expect("first acquire failed");
    assert_eq!(pool.active_count(), 1);

    // Second acquire should block because semaphore is exhausted
    let pool_clone = Arc::clone(&pool);
    let second_acquire = tokio::spawn(async move {
        pool_clone
            .acquire(SessionId::new(), make_session_state())
            .await
    });

    // Wait a bit — second acquire should still be pending
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!second_acquire.is_finished());
    assert_eq!(pool.active_count(), 1);

    // Release the first handle — second acquire should now complete
    drop(handle);
    let second_handle = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        second_acquire,
    )
    .await
    .expect("timed out waiting for second acquire")
    .expect("second acquire failed");

    assert_eq!(pool.active_count(), 1);
    drop(second_handle);
    assert_eq!(pool.active_count(), 0);
}

// ── Test: capacity tracking ───────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_capacity_tracking() {
    let pool = make_pool();
    assert_eq!(pool.capacity(), 10);
    assert_eq!(pool.active_count(), 0);

    let h1 = pool
        .acquire(SessionId::new(), make_session_state())
        .await
        .expect("acquire failed");
    assert_eq!(pool.active_count(), 1);
    assert_eq!(pool.capacity(), 10);

    drop(h1);
    assert_eq!(pool.active_count(), 0);
    assert_eq!(pool.capacity(), 10);
}

// ── Test: zero capacity rejects construction ──────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_zero_capacity_rejected() {
    let config = PoolConfig::new(noop_wasm_bytes(), PathBuf::from("/tmp/jadepaw-test"), 0);
    let result = InstancePool::new(config);
    assert!(result.is_err());
}

// ── Test: acquire latency benchmark (D-06) ────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn test_acquire_latency_benchmark() {
    let pool = make_pool();
    let iterations = 100;
    let mut durations = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let session_id = SessionId::new();
        let state = make_session_state();
        let start = Instant::now();
        let handle = pool
            .acquire(session_id, state)
            .await
            .expect("acquire failed");
        drop(handle);
        durations.push(start.elapsed());
    }

    let min = durations.iter().min().unwrap();
    let max = durations.iter().max().unwrap();
    let avg = durations.iter().sum::<std::time::Duration>() / iterations as u32;

    eprintln!("acquire+release benchmark (n={iterations}):");
    eprintln!("  min: {min:?}");
    eprintln!("  max: {max:?}");
    eprintln!("  avg: {avg:?}");

    // CI-friendly relaxed target: avg < 20ms
    // The 5ms P99 is the design target for production profiling.
    assert!(
        avg.as_millis() < 20,
        "average acquire+release latency {}ms exceeds 20ms threshold",
        avg.as_millis()
    );
}