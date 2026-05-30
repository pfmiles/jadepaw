//! Smoke tests for the wasmtime Engine lifecycle: create Engine, Store, load
//! a minimal guest module, and instantiate it. Proves the full vertical slice:
//! Engine -> Store -> Module -> Instance -> call _start.
//!
//! Per CLAUDE.md convention: all wasmtime tests MUST use
//! `#[tokio::test(flavor = "multi_thread")]`.

use jadepaw_wasm::EngineFactory;
use jadepaw_wasm::SessionState;
use jadepaw_core::{InstanceCapabilities, SessionId, TenantId};
use std::path::PathBuf;
use wasmtime::{Module, Store};

/// Full pipeline: EngineFactory -> Store -> Module -> Instance -> _start.
///
/// Proves that fuel is enabled (we can set_fuel without error) and that
/// the Store-per-session model works.
#[tokio::test(flavor = "multi_thread")]
async fn engine_smoke_full_pipeline() {
    let engine = EngineFactory::build().expect("EngineFactory::build should succeed");
    let session_id = SessionId::new();
    let tenant_id = TenantId::new();
    let capabilities = InstanceCapabilities::default();

    let session_state = SessionState::new(session_id, tenant_id, capabilities, PathBuf::from("/tmp"));
    let mut store = Store::new(&engine, session_state);

    // Register the ResourceLimiter (Pitfall 4 prevention)
    store.limiter(|s| &mut s.limits.hard_limit);

    // Fuel metering ON (Pitfall 1 prevention)
    store
        .set_fuel(1_000_000)
        .expect("set_fuel should succeed when consume_fuel is enabled");

    // Epoch deadline configured (Pitfall 5 prevention)
    store.epoch_deadline_async_yield_and_update(100);

    // Load the noop guest module
    // Use CARGO_MANIFEST_DIR to locate fixtures regardless of cwd
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let wat_path = std::path::Path::new(&manifest_dir).join("tests/fixtures/noop.wat");
    let wasm_bytes =
        wat::parse_file(&wat_path)
            .expect("noop.wat should parse");

    let module = Module::new(&engine, wasm_bytes).expect("Module::new should succeed");

    let instance = wasmtime::Instance::new_async(&mut store, &module, &[])
        .await
        .expect("Instance::new_async should succeed");

    let start_fn = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .expect("_start function should exist");

    start_fn
        .call_async(&mut store, ())
        .await
        .expect("_start should execute without error");
}

/// EngineFactory::build() must enable consume_fuel on the Engine config.
/// We verify this by checking that set_fuel works (it would fail if
/// consume_fuel is not enabled).
#[tokio::test(flavor = "multi_thread")]
async fn engine_has_fuel_enabled() {
    let engine = EngineFactory::build().expect("build engine");
    let state = SessionState::new(
        SessionId::new(),
        TenantId::new(),
        InstanceCapabilities::default(),
        PathBuf::from("/tmp"),
    );
    let mut store = Store::new(&engine, state);
    store.limiter(|s| &mut s.limits.hard_limit);

    // If consume_fuel is not enabled, this will panic.
    store
        .set_fuel(42)
        .expect("set_fuel should work when consume_fuel is true");

    let fuel = store.get_fuel().expect("get_fuel should work");
    assert_eq!(fuel, 42, "fuel should match what was set");
}

/// Verify SessionState is accessible from Store via data() and data_mut().
#[tokio::test(flavor = "multi_thread")]
async fn session_state_accessible_from_store() {
    let engine = EngineFactory::build().expect("build engine");
    let sid = SessionId::new();
    let tid = TenantId::new();
    let caps = InstanceCapabilities::default();

    let state = SessionState::new(sid, tid, caps, PathBuf::from("/tmp"));
    let mut store = Store::new(&engine, state);

    // Access via data()
    {
        let data = store.data();
        assert_eq!(data.session_id, sid);
        assert_eq!(data.tenant_id, tid);
        assert_eq!(data.capabilities.max_memory_mb, 64);
    }

    // Access via data_mut()
    {
        let data = store.data_mut();
        assert_eq!(data.session_id, sid);
    }
}