//! Skill repository trait — canonical persistence contract for skill metadata.
//!
//! The `SkillRepository` trait defines the data access interface for the
//! `skill_index` table. All methods require both `skill_id` and `tenant_id`
//! as mandatory parameters — the type system enforces isolation at every
//! call site (D-08, D-10).
//!
//! # Additive-only policy
//!
//! Methods may be added, never removed. CI must verify all implementors
//! cover every method.
//!
//! # Design (D-09)
//!
//! - The filesystem is the source of truth for skill content (SKILL.md files).
//! - The `skill_index` table is a cache for fast listing and lookup.
//! - `sync_index()` performs bulk upsert — called after walkdir scan at startup.
//! - Every method requires dual-key (skill_id, tenant_id) for multi-tenant isolation.
//! - `delete()` is idempotent — no error if the record doesn't exist.

use async_trait::async_trait;
use anyhow::Result;

use jadepaw_core::{SkillId, TenantId};

use crate::skill_models::{SkillIndexRecord, SkillIndexSummary};

/// Repository trait for skill metadata persistence.
///
/// All methods require both `skill_id` and `tenant_id` as mandatory
/// parameters — the type system enforces isolation at every call site (D-08, D-10).
#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// Bulk-upsert skill index entries.
    ///
    /// Performs INSERT OR REPLACE for each entry. Existing entries with the
    /// same `skill_id` and matching `tenant_id` are updated; entries with
    /// a different `tenant_id` for the same `skill_id` are rejected (dual-key
    /// isolation). Called after walkdir startup scan or on demand.
    async fn sync_index(&self, entries: &[SkillIndexRecord]) -> Result<()>;

    /// List all skills for a tenant (summary-only).
    ///
    /// Returns lightweight `SkillIndexSummary` records excluding the
    /// `tools_json` and `file_path` columns. Skills are ordered by
    /// `created_at` descending (most recent first).
    async fn list_by_tenant(&self, tenant_id: TenantId) -> Result<Vec<SkillIndexSummary>>;

    /// Look up a single skill by tenant and name.
    ///
    /// Returns the full `SkillIndexRecord` if found, or `None` if no skill
    /// matches both the `tenant_id` and `name`. Both must match — no
    /// cross-tenant leaks.
    async fn get_by_name(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Result<Option<SkillIndexRecord>>;

    /// Delete a skill from the index.
    ///
    /// Both `skill_id` and `tenant_id` must match for the deletion to
    /// succeed. Returns `Ok(())` even if the skill does not exist
    /// (idempotent delete).
    async fn delete(&self, skill_id: SkillId, tenant_id: TenantId) -> Result<()>;
}