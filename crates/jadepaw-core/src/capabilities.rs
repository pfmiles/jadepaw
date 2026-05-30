//! Capability-based security primitives.
//!
//! Defines `InstanceCapabilities` — the whitelist of operations a guest
//! Wasm instance is allowed to perform — and the pattern types used for
//! path and domain matching.

use crate::types::ToolId;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A pattern for matching file paths within the sandbox.
///
/// Wraps a glob-like string. At runtime, paths are matched against
/// `PathPattern` entries in the `can_read_files` and `can_write_files`
/// capability lists.
///
/// Examples: `"data/*"`, `"docs/**/*.md"`, `"config.json"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathPattern(pub String);

impl fmt::Display for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A pattern for matching network domains.
///
/// Wraps a glob-like string. At runtime, domains are matched against
/// `DomainPattern` entries in the `can_network_to` capability list.
///
/// Examples: `"api.example.com"`, `"*.example.com"`, `"*.svc.internal"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainPattern(pub String);

impl fmt::Display for DomainPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The capability whitelist for a guest Wasm instance.
///
/// Declared at instance initialization. **Default-deny** semantics (D-12):
/// if a capability is not explicitly granted, the corresponding can_* Vec
/// is empty and every check method returns `false`.
///
/// # Fields (D-10)
///
/// | Field | Purpose |
/// |-------|---------|
/// | `can_read_files` | Path patterns granting file read access |
/// | `can_write_files` | Path patterns granting file write access |
/// | `can_exec_tools` | Tool IDs the instance is allowed to call |
/// | `can_network_to` | Domain patterns granting network access |
/// | `max_memory_mb` | Per-instance memory cap in megabytes |
/// | `max_compute_units` | Per-instance compute budget |
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceCapabilities {
    /// File patterns granting read access.
    pub can_read_files: Vec<PathPattern>,
    /// File patterns granting write access.
    pub can_write_files: Vec<PathPattern>,
    /// Tools the instance is allowed to call.
    pub can_exec_tools: Vec<ToolId>,
    /// Domain patterns granting network access.
    pub can_network_to: Vec<DomainPattern>,
    /// Per-instance memory cap in megabytes.
    pub max_memory_mb: u32,
    /// Per-instance compute budget (units TBD).
    pub max_compute_units: u64,
}

impl Default for InstanceCapabilities {
    /// Default-deny: all capability lists are empty, `max_compute_units = 0`.
    /// `max_memory_mb` defaults to 64 to match the PoolingAllocator slot size.
    fn default() -> Self {
        Self {
            can_read_files: Vec::new(),
            can_write_files: Vec::new(),
            can_exec_tools: Vec::new(),
            can_network_to: Vec::new(),
            max_memory_mb: 64,
            max_compute_units: 0,
        }
    }
}