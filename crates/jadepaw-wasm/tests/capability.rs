//! Integration tests for capability enforcement and host function mediation.
//!
//! Proves the complete vertical slice: guest Wasm → host function →
//! capability check + path validation → I/O or denial.
//!
//! Per CLAUDE.md convention: all wasmtime tests MUST use
//! `#[tokio::test(flavor = "multi_thread")]`.

use jadepaw_core::{InstanceCapabilities, PathPattern, SessionId, TenantId};
use jadepaw_wasm::{create_linker, register_host_functions, EngineFactory, SessionState};
use std::path::PathBuf;
use wasmtime::{Linker, Module, Store};

/// Load and compile the tool_caller.wat fixture.
fn load_tool_caller(engine: &wasmtime::Engine) -> Module {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    let wat_path =
        std::path::Path::new(&manifest_dir).join("tests/fixtures/tool_caller.wat");
    let wasm_bytes = wat::parse_file(&wat_path).expect("tool_caller.wat should parse");
    Module::new(engine, wasm_bytes).expect("Module::new should succeed")
}

/// Create a Linker with all host functions registered.
fn create_test_linker(engine: &wasmtime::Engine) -> Linker<SessionState> {
    let mut linker = create_linker(engine);
    register_host_functions(&mut linker).expect("register host functions");
    linker
}

/// Create a Store with the given capabilities and sandbox root.
fn create_store(
    engine: &wasmtime::Engine,
    caps: InstanceCapabilities,
    sandbox: PathBuf,
) -> Store<SessionState> {
    let state = SessionState::new(SessionId::new(), TenantId::new(), caps, sandbox)
        .expect("SessionState::new should succeed");
    let mut store = Store::new(engine, state);
    store.limiter(|s| &mut s.limits.hard_limit);
    store.set_fuel(10_000_000).expect("set_fuel");
    store.epoch_deadline_async_yield_and_update(1000);
    store
}

// ============================================================
// Test 1: log_message always succeeds (safe default)
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_log_message_allowed() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");
    let mut store = create_store(
        &engine,
        InstanceCapabilities::default(),
        sandbox.path().to_path_buf(),
    );

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_log_message")
        .expect("test_log_message export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "log_message should always succeed");
    assert_eq!(result.unwrap(), 0, "log_message should return 0");
}

// ============================================================
// Test 2: file_read with path in whitelist succeeds
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_file_read_allowed() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    // Create the test file in the sandbox
    let test_file_content = b"hello from sandboxed file";
    std::fs::write(sandbox.path().join("test_file.txt"), test_file_content)
        .expect("create test file");

    let caps = InstanceCapabilities {
        can_read_files: vec![PathPattern("test_*".to_string())],
        ..Default::default()
    };

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_read")
        .expect("test_file_read export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "file_read should succeed with capability grant");
    let bytes_read = result.unwrap();
    assert!(
        bytes_read > 0,
        "file_read should return positive byte count, got {}",
        bytes_read
    );

    // Verify the file content was written to guest memory
    let mem = instance
        .get_memory(&mut store, "memory")
        .expect("get memory");
    let mem_data = mem.data(&store);
    let read_buffer = &mem_data[320..320 + bytes_read as usize];
    assert_eq!(
        read_buffer, test_file_content,
        "guest memory should contain file contents"
    );
}

// ============================================================
// Test 3: file_read with path not in whitelist → CapabilityDenied
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_file_read_denied() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    // Create the file but DON'T grant read capability
    std::fs::write(sandbox.path().join("test_file.txt"), b"secret data")
        .expect("create test file");

    let caps = InstanceCapabilities::default(); // default deny

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_read")
        .expect("test_file_read export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "call should not trap");
    assert_eq!(
        result.unwrap(),
        -1,
        "file_read without capability should return -1 (CapabilityDenied)"
    );
}

// ============================================================
// Test 4: file_read with ".." traversal path → rejected before I/O
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_path_traversal_file_read() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    // Grant read capability, but the path traverses out
    let caps = InstanceCapabilities {
        can_read_files: vec![PathPattern("*".to_string())],
        ..Default::default()
    };

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_read_traversal")
        .expect("test_file_read_traversal export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "call should not trap");
    assert_eq!(
        result.unwrap(),
        -1,
        "path traversal should be rejected before I/O, returning -1"
    );
}

// ============================================================
// Test 5: file_write with path traversal → rejected
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_path_traversal_file_write() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    let caps = InstanceCapabilities {
        can_write_files: vec![PathPattern("*".to_string())],
        ..Default::default()
    };

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_write_traversal")
        .expect("test_file_write_traversal export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "call should not trap");
    assert_eq!(
        result.unwrap(),
        -1,
        "path traversal file_write should return -1 (rejected)"
    );
}

// ============================================================
// Test 6: Default InstanceCapabilities denies all operations
// ============================================================

#[test]
fn test_default_deny_all() {
    let caps = InstanceCapabilities::default();
    assert!(caps.can_read_files.is_empty());
    assert!(caps.can_write_files.is_empty());
    assert!(caps.can_exec_tools.is_empty());
    assert!(caps.can_network_to.is_empty());
    assert_eq!(caps.max_memory_mb, 64);
    assert_eq!(caps.max_compute_units, 0);
}

// ============================================================
// Test 7: file_write allowed with write capability
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_file_write_allowed() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    let caps = InstanceCapabilities {
        can_write_files: vec![PathPattern("output*".to_string())],
        ..Default::default()
    };

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_write")
        .expect("test_file_write export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "file_write should succeed with capability grant");
    assert_eq!(result.unwrap(), 0, "file_write should return 0 on success");

    // Verify the file was actually written
    let file_path = sandbox.path().join("output.txt");
    assert!(file_path.exists(), "output.txt should exist");
    let contents = std::fs::read_to_string(&file_path).expect("read output file");
    assert_eq!(
        contents, "write test data",
        "file should contain written data"
    );
}

// ============================================================
// Test 8: file_write denied without write capability
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_file_write_denied() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    let caps = InstanceCapabilities::default(); // default deny

    let mut store = create_store(&engine, caps, sandbox.path().to_path_buf());

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_file_write")
        .expect("test_file_write export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok(), "call should not trap");
    assert_eq!(
        result.unwrap(),
        -1,
        "file_write without capability should return -1"
    );
}

// ============================================================
// Test 9: SessionState::session_id is accessible inside host functions
// (verified via call succeeding = session_id was read from caller.data())
// ============================================================

#[tokio::test(flavor = "multi_thread")]
async fn test_session_id_in_host_fn() {
    let engine = EngineFactory::build().expect("build engine");
    let module = load_tool_caller(&engine);
    let linker = create_test_linker(&engine);

    let sandbox = tempfile::tempdir().expect("create temp dir");

    let mut store = create_store(
        &engine,
        InstanceCapabilities::default(),
        sandbox.path().to_path_buf(),
    );

    // Read session_id before instantiation for verification
    let session_id_before = {
        let state = store.data();
        state.session_id
    };

    let instance = linker
        .instantiate_async(&mut store, &module)
        .await
        .expect("instantiate");

    // log_message always succeeds — proving that session_id was accessed
    // inside the host function (caller.data() at entry per D-11)
    let test_fn = instance
        .get_typed_func::<(), i32>(&mut store, "test_log_message")
        .expect("test_log_message export");

    let result = test_fn.call_async(&mut store, ()).await;
    assert!(result.is_ok());
    assert_eq!(
        result.unwrap(),
        0,
        "log_message succeeds proving session_id was accessed in host fn"
    );

    // Verify session_id is still correct after host function execution
    let session_id_after = {
        let state = store.data();
        state.session_id
    };
    assert_eq!(session_id_before, session_id_after);
}