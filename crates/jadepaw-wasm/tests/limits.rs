//! Tests for ResourceLimiter implementations: InstanceHardLimiter (64MB hard
//! cap) and TenantQuotaLimiter (aggregate budget), plus integration tests
//! proving memory/Fuel violations trap correctly.
//!
//! Per CLAUDE.md convention: all wasmtime tests MUST use
//! `#[tokio::test(flavor = "multi_thread")]`.

use jadepaw_core::{InstanceCapabilities, SessionId, TenantId};
use jadepaw_wasm::{
    EngineFactory, InstanceHardLimiter, SessionLimits, SessionState, TenantQuotaLimiter,
};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use wasmtime::{Module, ResourceLimiter, Store};

/// InstanceHardLimiter::memory_growing returns Ok(true) when desired <= 64MB.
#[test]
fn instance_hard_limiter_allows_within_limit() {
    let mut limiter = InstanceHardLimiter::new(64);
    let result = limiter.memory_growing(0, 64 * 1024 * 1024, None);
    assert!(
        result.is_ok(),
        "memory_growing should return Ok when within limit"
    );
    assert!(result.unwrap(), "should return Ok(true) to allow growth");
}

/// InstanceHardLimiter::memory_growing returns Err() when desired > 64MB.
#[test]
fn instance_hard_limiter_traps_above_limit() {
    let mut limiter = InstanceHardLimiter::new(64);
    let result = limiter.memory_growing(0, 64 * 1024 * 1024 + 1, None);
    assert!(
        result.is_err(),
        "memory_growing should return Err() when exceeding 64MB hard cap"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("limit exceeded"),
        "error message should indicate limit exceeded, got: {err_msg}"
    );
}

/// TenantQuotaLimiter::memory_growing returns Ok(false) when tenant
/// aggregate budget is exceeded (recoverable — guest gets -1).
#[test]
fn tenant_quota_limiter_denies_above_aggregate() {
    let inner = InstanceHardLimiter::new(64);
    let budget_used = Arc::new(AtomicUsize::new(0));
    // Tenant has 1MB total budget, already used 0.9MB. Grow by 0.2MB -> 1.1MB > 1MB.
    let mut limiter = TenantQuotaLimiter::new_with_budget(1, budget_used.clone(), inner);

    // Pre-set the budget tracker to near limit
    budget_used.store(900 * 1024, std::sync::atomic::Ordering::Relaxed);

    let result = limiter.memory_growing(0, 200 * 1024, None);
    assert!(
        result.is_ok(),
        "Ok(false) is not an Err — it's a recoverable deny"
    );
    assert!(!result.unwrap(), "should return Ok(false) when budget exceeded");
}

/// TenantQuotaLimiter delegates to inner InstanceHardLimiter for
/// per-instance hard cap enforcement.
#[test]
fn tenant_quota_delegates_to_inner_hard_cap() {
    let inner = InstanceHardLimiter::new(64);
    let budget_used = Arc::new(AtomicUsize::new(0));
    let mut limiter = TenantQuotaLimiter::new_with_budget(1024, budget_used, inner);

    // Even with tons of budget, inner hard cap at 64MB still catches >64MB
    let result = limiter.memory_growing(0, 64 * 1024 * 1024 + 1, None);
    assert!(
        result.is_err(),
        "inner InstanceHardLimiter should trap on >64MB"
    );
}

/// Integration test: guest WAT that grows memory beyond 64MB traps.
/// Uses SessionState's limiter registration pattern.
#[tokio::test(flavor = "multi_thread")]
async fn guest_memory_grow_beyond_64mb_traps() {
    let engine = EngineFactory::build().expect("build engine");

    // WAT that tries to grow memory to 500 pages (each page = 64KB)
    // 500 * 64KB = 32MB — actually well within 64MB. Let's use 2000 pages.
    // 2000 * 64KB = 128MB > 64MB limit.
    let wat_src = r#"
    (module
      (memory (export "memory") 1)
      (func (export "grow_memory") (result i32)
        i32.const 2000
        memory.grow
        return
      )
      (func (export "_start"))
    )
    "#;
    let wasm_bytes = wat::parse_str(wat_src).expect("WAT should parse");

    let module = Module::new(&engine, wasm_bytes).expect("Module::new");
    let state = SessionState::new(
        SessionId::new(),
        TenantId::new(),
        InstanceCapabilities::default(),
    );
    let mut store = Store::new(&engine, state);
    store.limiter(|s| &mut s.limits.hard_limit);
    store.set_fuel(1_000_000).expect("set_fuel");
    store
        .epoch_deadline_async_yield_and_update(100)
        .expect("epoch_deadline");

    let instance =
        wasmtime::Instance::new_async(&mut store, &module, &[])
            .await
            .expect("instantiate");

    let grow_fn = instance
        .get_typed_func::<(), i32>(&mut store, "grow_memory")
        .expect("grow_memory function");

    let result = grow_fn.call_async(&mut store, ()).await;
    // The memory.grow will be intercepted by InstanceHardLimiter, which returns
    // Err() for >64MB. This should trap (either via wasm trap or our Err).
    assert!(
        result.is_err(),
        "growing memory beyond 64MB should result in an error"
    );
}

/// Integration test: guest WAT with infinite loop traps from Fuel exhaustion.
#[tokio::test(flavor = "multi_thread")]
async fn guest_infinite_loop_traps_from_fuel_exhaustion() {
    let engine = EngineFactory::build().expect("build engine");

    // WAT with an infinite loop via `(loop (br 0))`
    let wat_src = r#"
    (module
      (memory (export "memory") 1)
      (func (export "infinite_loop")
        (loop (br 0))
      )
      (func (export "_start"))
    )
    "#;
    let wasm_bytes = wat::parse_str(wat_src).expect("WAT should parse");

    let module = Module::new(&engine, wasm_bytes).expect("Module::new");
    let state = SessionState::new(
        SessionId::new(),
        TenantId::new(),
        InstanceCapabilities::default(),
    );
    let mut store = Store::new(&engine, state);
    store.limiter(|s| &mut s.limits.hard_limit);
    // Tiny fuel budget — 1000 units should run out fast
    store.set_fuel(1_000).expect("set_fuel");
    store
        .epoch_deadline_async_yield_and_update(100)
        .expect("epoch_deadline");

    let instance =
        wasmtime::Instance::new_async(&mut store, &module, &[])
            .await
            .expect("instantiate");

    let loop_fn = instance
        .get_typed_func::<(), ()>(&mut store, "infinite_loop")
        .expect("infinite_loop function");

    let result = loop_fn.call_async(&mut store, ()).await;
    assert!(
        result.is_err(),
        "infinite loop should trap from fuel exhaustion"
    );
}

/// Unit test: SessionState new() creates state with correct fields.
#[test]
fn session_state_fields() {
    let sid = SessionId::new();
    let tid = TenantId::new();
    let caps = InstanceCapabilities::default();

    let state = SessionState::new(sid, tid, caps.clone());

    assert_eq!(state.session_id, sid);
    assert_eq!(state.tenant_id, tid);
    assert_eq!(state.capabilities, caps);
    // created_at should be roughly now
    let now = chrono::Utc::now();
    let diff = now - state.created_at;
    assert!(
        diff.num_seconds() < 10,
        "created_at should be close to now"
    );
}

/// SessionLimits contains an InstanceHardLimiter initialized from
/// InstanceCapabilities::max_memory_mb.
#[test]
fn session_limits_from_capabilities() {
    let mut caps = InstanceCapabilities::default();
    caps.max_memory_mb = 128;
    let state = SessionState::new(SessionId::new(), TenantId::new(), caps);

    // SessionLimits should have been created with the capabilities' max_memory_mb
    // We can't easily introspect InstanceHardLimiter, but we can test the limiter
    let mut limiter = state.limits.hard_limit;
    // 128MB = 128 * 1024 * 1024 bytes
    let result = limiter.memory_growing(0, 128 * 1024 * 1024, None);
    assert!(result.is_ok(), "128MB should be allowed with 128MB cap");
    let result2 = limiter.memory_growing(0, 128 * 1024 * 1024 + 1, None);
    assert!(result2.is_err(), ">128MB should be denied with 128MB cap");
}