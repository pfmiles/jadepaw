//! Tests for InstanceCapabilities, PathPattern, DomainPattern
//!
//! Verifies default-deny semantics (D-12) and all required fields (D-10).

use jadepaw_core::DomainPattern;
use jadepaw_core::InstanceCapabilities;
use jadepaw_core::PathPattern;

#[test]
fn capabilities_default_is_deny_all() {
    let caps = InstanceCapabilities::default();

    // Default deny: all can_* Vec fields must be empty (D-12)
    assert!(
        caps.can_read_files.is_empty(),
        "default should deny all file reads"
    );
    assert!(
        caps.can_write_files.is_empty(),
        "default should deny all file writes"
    );
    assert!(
        caps.can_exec_tools.is_empty(),
        "default should deny all tool execution"
    );
    assert!(
        caps.can_network_to.is_empty(),
        "default should deny all network access"
    );

    // Default values for limit fields
    assert_eq!(caps.max_memory_mb, 64, "default max_memory_mb should be 64");
    assert_eq!(
        caps.max_compute_units, 0,
        "default max_compute_units should be 0"
    );
}

#[test]
fn capabilities_with_explicit_values() {
    let caps = InstanceCapabilities {
        can_read_files: vec![PathPattern("data/*".to_string())],
        can_write_files: vec![],
        can_exec_tools: vec![],
        can_network_to: vec![DomainPattern("api.example.com".to_string())],
        max_memory_mb: 128,
        max_compute_units: 1000,
    };

    assert_eq!(caps.can_read_files.len(), 1);
    assert_eq!(caps.can_read_files[0].0, "data/*");
    assert_eq!(caps.can_network_to.len(), 1);
    assert_eq!(caps.can_network_to[0].0, "api.example.com");
    assert_eq!(caps.max_memory_mb, 128);
    assert_eq!(caps.max_compute_units, 1000);
}

#[test]
fn path_pattern_is_newtype_wrapping_string() {
    let pp = PathPattern("docs/**".to_string());
    assert_eq!(pp.0, "docs/**");

    // Verify clone
    let pp2 = pp.clone();
    assert_eq!(pp2.0, "docs/**");
}

#[test]
fn domain_pattern_is_newtype_wrapping_string() {
    let dp = DomainPattern("*.example.com".to_string());
    assert_eq!(dp.0, "*.example.com");

    // Verify clone
    let dp2 = dp.clone();
    assert_eq!(dp2.0, "*.example.com");
}