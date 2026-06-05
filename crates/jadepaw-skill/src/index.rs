//! SQLite skill index cache operations.
//!
//! The `SkillIndex` wraps a `SkillRepository` and provides higher-level
//! operations: parsing SKILL.md files into index records and syncing them
//! to the SQLite cache. The filesystem remains the source of truth (D-09);
//! the SQLite `skill_index` table is a cache for fast listing.
//!
//! # Design (D-09)
//!
//! - `sync()` parses each SKILL.md on disk and calls `repo.sync_index()`.
//! - Invalid SKILL.md files are logged with `tracing::warn!` and skipped.
//! - One broken skill does not block others from being indexed.
//! - `list_by_tenant()` and `get_by_name()` delegate to the repository.

use anyhow::{Context, Result};
use std::sync::Arc;

use jadepaw_core::{SkillId, SkillManifest, TenantId};

use jadepaw_db::{SkillIndexRecord, SkillIndexSummary, SkillRepository};

use crate::loader::{parse_tenant_id_str, SkillFileEntry};
use crate::parser::parse_skill_file;

/// High-level skill index that wraps a repository and provides parse+sync
/// operations.
///
/// Owns a reference-counted `SkillRepository` trait object so it can be
/// shared across components.
pub struct SkillIndex {
    repo: Arc<dyn SkillRepository>,
}

impl SkillIndex {
    /// Create a new skill index backed by the given repository.
    pub fn new(repo: Arc<dyn SkillRepository>) -> Self {
        Self { repo }
    }

    /// Parse and sync skill file entries to the SQLite index.
    ///
    /// For each `SkillFileEntry`:
    /// 1. Read the SKILL.md file content via `tokio::fs::read_to_string`
    /// 2. Parse with `parse_skill_file()` to validate
    /// 3. On success: build a `SkillIndexRecord` and add to the batch
    /// 4. On failure: log a warning via `tracing::warn!` and continue
    ///
    /// After all entries are processed, calls `repo.sync_index()` with the
    /// collected records. The repository's dual-key isolation ensures
    /// tenant boundaries are preserved.
    pub async fn sync(&self, entries: &[SkillFileEntry]) -> Result<()> {
        let mut records = Vec::with_capacity(entries.len());

        for entry in entries {
            let content = match tokio::fs::read_to_string(&entry.path).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(
                        path = %entry.path.display(),
                        error = %e,
                        "failed to read skill file"
                    );
                    continue;
                }
            };

            let tenant_id = match parse_tenant_id_str(&entry.tenant_id_str) {
                Some(id) => id,
                None => {
                    tracing::warn!(
                        tenant_id_str = %entry.tenant_id_str,
                        path = %entry.path.display(),
                        "invalid tenant_id directory name"
                    );
                    continue;
                }
            };

            match parse_skill_file(&content, &entry.skill_name, &entry.path) {
                Ok((manifest, _body)) => {
                    let record = build_index_record(
                        manifest,
                        tenant_id,
                        entry.path.display().to_string(),
                    );
                    records.push(record);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %entry.path.display(),
                        error = %e,
                        "failed to parse skill file, skipping"
                    );
                }
            }
        }

        if !records.is_empty() {
            self.repo
                .sync_index(&records)
                .await
                .context("failed to sync skill index")?;
        }

        Ok(())
    }

    /// List all skills for a tenant (summary-only).
    ///
    /// Delegates to `SkillRepository::list_by_tenant()`.
    pub async fn list_by_tenant(&self, tenant_id: TenantId) -> Result<Vec<SkillIndexSummary>> {
        self.repo
            .list_by_tenant(tenant_id)
            .await
            .context("failed to list skills by tenant")
    }

    /// Look up a single skill by tenant and name.
    ///
    /// Delegates to `SkillRepository::get_by_name()`.
    pub async fn get_by_name(
        &self,
        tenant_id: TenantId,
        name: &str,
    ) -> Result<Option<SkillIndexRecord>> {
        self.repo
            .get_by_name(tenant_id, name)
            .await
            .context("failed to get skill by name")
    }
}

/// Build a `SkillIndexRecord` from a parsed `SkillManifest`.
fn build_index_record(
    manifest: SkillManifest,
    tenant_id: TenantId,
    file_path: String,
) -> SkillIndexRecord {
    let tools_json = serde_json::to_string(&manifest.tools).unwrap_or_else(|_| "[]".to_string());
    let now = chrono::Utc::now();

    SkillIndexRecord {
        skill_id: SkillId::new(),
        tenant_id,
        name: manifest.name,
        description: manifest.description,
        version: manifest.version,
        tools_json,
        file_path,
        created_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillId;
    use std::io::Write;

    fn create_test_skill_file(dir: &std::path::Path, name: &str) -> SkillFileEntry {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let path = skill_dir.join("SKILL.md");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            "---\nname: {}\ndescription: Test skill {}\n---\n# Body\n",
            name, name
        )
        .unwrap();
        SkillFileEntry {
            path,
            skill_name: name.to_string(),
            tenant_id_str: "global".to_string(),
        }
    }

    #[test]
    fn build_index_record_creates_valid_record() {
        let manifest = SkillManifest {
            name: "test-skill".to_string(),
            description: "A test".to_string(),
            tools: vec!["read_file".to_string()],
            constraints: None,
            version: Some("1.0.0".to_string()),
            author: None,
            metadata: None,
            source_path: std::path::PathBuf::from("/tmp/SKILL.md"),
        };

        let record = build_index_record(
            manifest,
            TenantId::new(),
            "/tmp/test-skill/SKILL.md".to_string(),
        );

        assert_eq!(record.name, "test-skill");
        assert_eq!(record.description, "A test");
        assert_eq!(record.version, Some("1.0.0".to_string()));
        assert!(record.tools_json.contains("read_file"));
        assert_eq!(record.file_path, "/tmp/test-skill/SKILL.md");
        // Verify skill_id is non-zero
        assert!(*record.skill_id != uuid::Uuid::nil());
    }

    #[test]
    fn build_index_record_empty_tools_defaults_to_empty_array() {
        let manifest = SkillManifest {
            name: "minimal".to_string(),
            description: "Min".to_string(),
            tools: vec![],
            constraints: None,
            version: None,
            author: None,
            metadata: None,
            source_path: std::path::PathBuf::from("/tmp/SKILL.md"),
        };

        let record = build_index_record(
            manifest,
            TenantId::new(),
            "/tmp/SKILL.md".to_string(),
        );

        assert_eq!(record.tools_json, "[]");
    }
}