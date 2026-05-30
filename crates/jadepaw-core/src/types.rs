//! Core types shared across jadepaw crates.
//!
//! Defines strongly-typed identifiers using UUID v7 for time-ordered uniqueness.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use uuid::Uuid;

/// Unique identifier for a session.
///
/// Uses UUID v7 (time-ordered) for database index friendliness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Create a new session identifier using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Deref for SessionId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for a tenant.
///
/// Uses UUID v7 (time-ordered) for database index friendliness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(Uuid);

impl TenantId {
    /// Create a new tenant identifier using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Deref for TenantId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for TenantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for TenantId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for a tool.
///
/// Uses UUID v7 (time-ordered) for database index friendliness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolId(Uuid);

impl ToolId {
    /// Create a new tool identifier using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Deref for ToolId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for ToolId {
    fn default() -> Self {
        Self::new()
    }
}