//! Compile-time verification of the HostFunctions trait contract.
//!
//! Per D-01: The HostFunctions trait must be CI-verifiable — any implementor
//! must cover all methods. Rust's trait system enforces this at compile time,
//! so the test is a compile-pass check: define a struct that implements
//! HostFunctions, verify it compiles, and verify the methods return the
//! correct Result types.

use async_trait::async_trait;
use jadepaw_core::{HostFunctions, JadepawError, Result};

/// A test implementor of HostFunctions that returns fixed values.
/// This proves the trait is implementable by downstream crates without
/// depending on jadepaw-wasm.
struct TestHostFn;

#[async_trait]
impl HostFunctions for TestHostFn {
    async fn log_message(&self, _level: String, _message: String) -> Result<()> {
        Ok(())
    }

    async fn file_read(&self, _path: String) -> Result<Vec<u8>> {
        Err(JadepawError::CapabilityDenied {
            operation: "file_read".to_string(),
            detail: "test stub".to_string(),
        })
    }

    async fn file_write(&self, _path: String, _data: Vec<u8>) -> Result<()> {
        Err(JadepawError::CapabilityDenied {
            operation: "file_write".to_string(),
            detail: "test stub".to_string(),
        })
    }
}

/// Verify that a struct implementing HostFunctions compiles.
/// If a method is added to the trait, this test will fail to compile
/// until TestHostFn is updated — enforcing CI-verifiability per D-01.
#[test]
fn test_host_functions_trait_is_implementable() {
    // Simply constructing the struct proves the trait impl compiles.
    let _host = TestHostFn;
}

/// Verify that the trait methods return correct Result types.
#[test]
fn test_host_functions_trait_result_types() {
    let host = TestHostFn;
    let rt = tokio::runtime::Runtime::new().unwrap();

    // log_message returns Result<()>
    let result: Result<()> = rt.block_on(host.log_message("info".into(), "test".into()));
    assert!(result.is_ok());

    // file_read returns Result<Vec<u8>>
    let result: Result<Vec<u8>> = rt.block_on(host.file_read("/tmp/test".into()));
    assert!(result.is_err());
    match result.unwrap_err() {
        JadepawError::CapabilityDenied { operation, .. } => {
            assert_eq!(operation, "file_read");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }

    // file_write returns Result<()>
    let result: Result<()> =
        rt.block_on(host.file_write("/tmp/test".into(), vec![1, 2, 3]));
    assert!(result.is_err());
    match result.unwrap_err() {
        JadepawError::CapabilityDenied { operation, .. } => {
            assert_eq!(operation, "file_write");
        }
        other => panic!("expected CapabilityDenied, got {other:?}"),
    }
}