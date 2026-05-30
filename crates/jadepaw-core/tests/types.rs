//! Tests for core types: SessionId, TenantId, ToolId
//!
//! These are newtype wrappers around uuid::Uuid (v7).

use jadepaw_core::SessionId;
use jadepaw_core::TenantId;
use jadepaw_core::ToolId;

#[test]
fn session_id_new_creates_unique_ids() {
    let sid1 = SessionId::new();
    let sid2 = SessionId::new();
    assert_ne!(sid1, sid2, "SessionId::new() should produce unique ids");
}

#[test]
fn tenant_id_new_creates_unique_ids() {
    let tid1 = TenantId::new();
    let tid2 = TenantId::new();
    assert_ne!(tid1, tid2, "TenantId::new() should produce unique ids");
}

#[test]
fn tool_id_new_creates_unique_ids() {
    let tool1 = ToolId::new();
    let tool2 = ToolId::new();
    assert_ne!(tool1, tool2, "ToolId::new() should produce unique ids");
}

#[test]
fn session_id_is_newtype_wrapping_uuid() {
    let sid = SessionId::new();
    // Can access the inner uuid via deref
    let _inner: &uuid::Uuid = &*sid;
}

#[test]
fn session_id_has_display() {
    let sid = SessionId::new();
    let s = format!("{sid}");
    // UUID v7 display format has hyphens
    assert!(!s.is_empty());
    assert!(s.contains('-'), "expected UUID format with hyphens, got: {s}");
}