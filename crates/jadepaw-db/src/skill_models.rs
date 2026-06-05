//! Skill data models: `SkillIndexRecord` and `SkillIndexSummary`.
//!
//! These types form the skill persistence layer's data contract. All types derive
//! `Serialize`/`Deserialize` for JSON blob serialization and wire transport.
//!
//! # Design (D-09)
//!
//! - `SkillIndexRecord` contains the full skill metadata cached in the `skill_index`
//!   table. The filesystem remains the source of truth — this is a cache for fast listing.
//! - `SkillIndexSummary` is a lightweight subset used for `list_by_tenant()`.
//! - Both types carry `skill_id` and `tenant_id` for dual-key isolation (D-10).

use jadepaw_core::{SkillId, TenantId};
use serde::{Deserialize, Serialize};

/// A full skill metadata record cached in the SQLite `skill_index` table.
///
/// Contains all fields from the parsed SKILL.md manifest plus the file path
/// and timestamps. The `tools_json` field is a pre-serialized JSON array of
/// tool name strings — the caller is responsible for calling
/// `serde_json::to_string()` before constructing this struct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillIndexRecord {
    /// Unique skill identifier (UUID v7).
    pub skill_id: SkillId,
    /// Owning tenant identifier.
    pub tenant_id: TenantId,
    /// Skill name (must match directory name, kebab-case).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Semantic version string, if declared.
    pub version: Option<String>,
    /// JSON-serialized array of declared tool names.
    pub tools_json: String,
    /// Absolute path to the SKILL.md file on disk.
    pub file_path: String,
    /// Record creation timestamp.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp.
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A lightweight skill summary (subset of `SkillIndexRecord` fields).
///
/// Used for `list_by_tenant()` to avoid loading the full record when only
/// metadata is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillIndexSummary {
    /// Unique skill identifier.
    pub skill_id: SkillId,
    /// Owning tenant identifier.
    pub tenant_id: TenantId,
    /// Skill name.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Semantic version string, if declared.
    pub version: Option<String>,
}