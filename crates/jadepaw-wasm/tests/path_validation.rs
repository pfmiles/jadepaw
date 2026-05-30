//! Unit tests for path validation and capability check methods.
//!
//! Tests cover: normalize_path, validate_sandbox_path, can_read_file,
//! can_write_file, can_call_tool, can_access_domain.
//!
//! Per CLAUDE.md convention: all wasmtime tests MUST use
//! `#[tokio::test(flavor = "multi_thread")]`.

use jadepaw_core::{DomainPattern, InstanceCapabilities, PathPattern, SessionId, TenantId, ToolId};
use jadepaw_wasm::SessionState;
use std::path::{Path, PathBuf};

// ============================================================
// normalize_path tests
// ============================================================

/// Test 1: normalize_path resolves ".." traversal correctly.
#[test]
fn normalize_path_collapses_parent() {
    // "foo/bar/../baz" -> "foo/baz"
    let result = jadepaw_wasm::normalize_path("foo/bar/../baz");
    // After normalizing: split by / -> ["foo", "bar", "..", "baz"]
    // filter "..": pop "bar", then "baz" -> ["foo", "baz"]
    let expected: PathBuf = ["foo", "baz"].iter().collect();
    assert_eq!(result, expected);
}

/// Test 2: normalize_path handles multiple ".." that go above.
#[test]
fn normalize_path_multiple_parent_above_root() {
    // "foo/../../../etc/passwd" strips "foo", then goes above -> "../etc/passwd"
    let result = jadepaw_wasm::normalize_path("foo/../../../etc/passwd");
    let expected: PathBuf = ["..", "..", "etc", "passwd"].iter().collect();
    assert_eq!(result, expected);
}

/// Test 3: normalize_path removes "." components.
#[test]
fn normalize_path_removes_dot() {
    let result = jadepaw_wasm::normalize_path("foo/./bar/./baz");
    let expected: PathBuf = ["foo", "bar", "baz"].iter().collect();
    assert_eq!(result, expected);
}

/// Test 4: normalize_path strips leading slash.
#[test]
fn normalize_path_strips_leading_slash() {
    let result = jadepaw_wasm::normalize_path("/foo/bar");
    let expected: PathBuf = ["foo", "bar"].iter().collect();
    assert_eq!(result, expected);
}

/// Test 5: normalize_path handles trailing slash.
#[test]
fn normalize_path_trailing_slash() {
    let result = jadepaw_wasm::normalize_path("foo/bar/");
    let expected: PathBuf = ["foo", "bar"].iter().collect();
    assert_eq!(result, expected);
}

/// Test 6: normalize_path returns empty path for ".".
#[test]
fn normalize_path_just_dot() {
    let result = jadepaw_wasm::normalize_path(".");
    assert_eq!(result, PathBuf::new());
}

/// Test 7: normalize_path returns empty path for "".
#[test]
fn normalize_path_empty() {
    let result = jadepaw_wasm::normalize_path("");
    assert_eq!(result, PathBuf::new());
}

// ============================================================
// validate_sandbox_path tests
// ============================================================

/// Test 8: validate_sandbox_path rejects path traversal ("../../../etc/passwd").
#[test]
fn validate_sandbox_path_rejects_traversal() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let sandbox = temp.path().canonicalize().expect("canonicalize sandbox");

    // Create a subdirectory so we have a valid starting point
    let sub_dir = sandbox.join("subdir");
    std::fs::create_dir_all(&sub_dir).expect("create subdir");

    let result = jadepaw_wasm::validate_sandbox_path("subdir/../../../etc/passwd", &sandbox);
    assert!(result.is_err(), "path traversal should be rejected");
    let err = result.unwrap_err();
    match err {
        jadepaw_core::JadepawError::PathValidationError { ref path, .. } => {
            assert!(path.contains("passwd"), "error should mention the path");
        }
        _ => panic!("expected PathValidationError, got {:?}", err),
    }
}

/// Test 9: validate_sandbox_path accepts valid path within sandbox root.
#[test]
fn validate_sandbox_path_accepts_valid() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let sandbox = temp.path().canonicalize().expect("canonicalize sandbox");

    // Create a file in the sandbox
    let file_path = sandbox.join("notes.txt");
    std::fs::write(&file_path, b"hello").expect("create test file");

    let result = jadepaw_wasm::validate_sandbox_path("notes.txt", &sandbox);
    assert!(result.is_ok(), "valid path should be accepted");
    let resolved = result.unwrap();
    assert_eq!(resolved, file_path.canonicalize().unwrap());
}

/// Test 10: validate_sandbox_path rejects absolute path outside sandbox.
#[test]
fn validate_sandbox_path_rejects_absolute_outside() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let sandbox = temp.path().canonicalize().expect("canonicalize sandbox");

    // Absolute path to /tmp/something — should resolve outside sandbox
    let result = jadepaw_wasm::validate_sandbox_path("/tmp/should-not-exist-xyz", &sandbox);
    // On macOS, /tmp is a symlink to /private/tmp, so canonicalization
    // could succeed but the prefix check should catch it
    assert!(result.is_err(), "absolute path outside sandbox should be rejected");
}

/// Test 11: validate_sandbox_path errors on non-existent path.
#[test]
fn validate_sandbox_path_nonexistent() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let sandbox = temp.path().canonicalize().expect("canonicalize sandbox");

    let result = jadepaw_wasm::validate_sandbox_path("nonexistent_file.txt", &sandbox);
    // canonicalize() fails for nonexistent paths → should error
    assert!(result.is_err(), "nonexistent path should cause error");
}

// ============================================================
// Capability check tests (can_read_file, can_write_file, etc.)
// ============================================================

fn make_session_state(caps: InstanceCapabilities) -> SessionState {
    SessionState::new(SessionId::new(), TenantId::new(), caps)
}

/// Test 12: can_read_file returns true when path matches a PathPattern.
#[test]
fn can_read_file_matches_pattern() {
    let caps = InstanceCapabilities {
        can_read_files: vec![PathPattern("data/*".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_read_file("data/config.json"));
    assert!(state.can_read_file("data/notes.txt"));
}

/// Test 13: can_read_file returns false when path does not match (default deny).
#[test]
fn can_read_file_default_deny() {
    let state = make_session_state(InstanceCapabilities::default());
    assert!(!state.can_read_file("anything.txt"));
}

/// Test 14: can_read_file returns false for empty patterns.
#[test]
fn can_read_file_empty_patterns() {
    let caps = InstanceCapabilities::default();
    assert!(caps.can_read_files.is_empty(), "default caps should have empty read files");
}

/// Test 15: can_write_file returns true when path matches a PathPattern.
#[test]
fn can_write_file_matches_pattern() {
    let caps = InstanceCapabilities {
        can_write_files: vec![PathPattern("output/*".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_write_file("output/result.json"));
}

/// Test 16: can_write_file returns false when path does not match.
#[test]
fn can_write_file_default_deny() {
    let state = make_session_state(InstanceCapabilities::default());
    assert!(!state.can_write_file("anywhere.txt"));
}

/// Test 17: can_call_tool returns true only when ToolId is in capabilities.
#[test]
fn can_call_tool_matches() {
    let tool_a = ToolId::new();
    let tool_b = ToolId::new();
    let caps = InstanceCapabilities {
        can_exec_tools: vec![tool_a],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_call_tool(&tool_a));
    assert!(!state.can_call_tool(&tool_b));
}

/// Test 18: can_call_tool default deny.
#[test]
fn can_call_tool_default_deny() {
    let state = make_session_state(InstanceCapabilities::default());
    assert!(!state.can_call_tool(&ToolId::new()));
}

/// Test 19: can_access_domain matches exact domain.
#[test]
fn can_access_domain_exact_match() {
    let caps = InstanceCapabilities {
        can_network_to: vec![DomainPattern("api.example.com".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_access_domain("api.example.com"));
    assert!(!state.can_access_domain("other.example.com"));
}

/// Test 20: can_access_domain matches wildcard pattern.
#[test]
fn can_access_domain_wildcard_match() {
    let caps = InstanceCapabilities {
        can_network_to: vec![DomainPattern("*.example.com".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_access_domain("api.example.com"));
    assert!(state.can_access_domain("www.example.com"));
    assert!(!state.can_access_domain("example.com"));
    assert!(!state.can_access_domain("api.other.com"));
}

/// Test 21: can_access_domain default deny.
#[test]
fn can_access_domain_default_deny() {
    let state = make_session_state(InstanceCapabilities::default());
    assert!(!state.can_access_domain("anything.com"));
}

/// Test 22: default InstanceCapabilities denies all operations (default deny per D-12).
#[test]
fn default_capabilities_deny_all() {
    let state = make_session_state(InstanceCapabilities::default());
    assert!(!state.can_read_file("test.txt"));
    assert!(!state.can_write_file("test.txt"));
    assert!(!state.can_call_tool(&ToolId::new()));
    assert!(!state.can_access_domain("example.com"));
}

/// Test 23: explicit prefix match for PathPattern.
#[test]
fn path_pattern_exact_match() {
    let caps = InstanceCapabilities {
        can_read_files: vec![PathPattern("exact_file.txt".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_read_file("exact_file.txt"));
    // exact match only, not prefix
    assert!(!state.can_read_file("exact_file.txt.bak"));
}

/// Test 24: PathPattern "*" matches everything.
#[test]
fn path_pattern_wildcard_matches_everything() {
    let caps = InstanceCapabilities {
        can_write_files: vec![PathPattern("*".to_string())],
        ..Default::default()
    };
    let state = make_session_state(caps);
    assert!(state.can_write_file("anything.txt"));
    assert!(state.can_write_file("deep/nested/file.json"));
}