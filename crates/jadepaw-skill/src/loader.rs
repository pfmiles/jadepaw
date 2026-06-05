//! Filesystem scanner that discovers all SKILL.md files under a skills root.
//!
//! Uses `walkdir` to traverse the directory tree. The scan is synchronous
//! (blocking I/O) and MUST be called via `tokio::task::spawn_blocking` at
//! the call site to avoid starving the async runtime (RESEARCH.md Pitfall 4).
//!
//! # Directory layout (D-10)
//!
//! ```text
//! <skills_root>/
//!   global/               # Built-in skills available to all tenants
//!     my-skill/           # Skill directory (name = directory name)
//!       SKILL.md           # Skill definition
//!   <tenant_id>/          # Per-tenant private skills
//!     custom-skill/
//!       SKILL.md
//! ```
//!
//! # Tenancy (D-10)
//!
//! - `global/` directory: skills available to all tenants. Mapped to
//!   `TenantId::nil()` (all-zeros UUID) in the index.
//! - `<tenant_id>/` directory: private skills for a specific tenant.
//!   The directory name is parsed as a UUID.

use jadepaw_core::TenantId;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// An entry discovered by walking the skills directory tree.
///
/// Each entry corresponds to one SKILL.md file found on disk. The
/// `tenant_id_str` is the raw directory name (either "global" or a
/// UUID string) — callers must convert it to a proper `TenantId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFileEntry {
    /// Full path to the SKILL.md file.
    pub path: PathBuf,
    /// The skill name, derived from the parent directory name.
    pub skill_name: String,
    /// The tenant directory name ("global" or a UUID string).
    pub tenant_id_str: String,
}

/// Scanner that discovers SKILL.md files under a skills root directory.
///
/// The `skills_root` is typically `~/.jadepaw/skills/`. The scanner walks
/// the entire tree, finds every SKILL.md file, and returns metadata about
/// each one (path, skill name, tenant scope).
pub struct SkillLoader {
    skills_root: PathBuf,
}

impl SkillLoader {
    /// Create a new skill loader for the given skills root directory.
    pub fn new(skills_root: PathBuf) -> Self {
        Self { skills_root }
    }

    /// Scan the skills root and return all discovered SKILL.md file entries.
    ///
    /// Synchronous (blocking I/O). Callers MUST wrap this in
    /// `tokio::task::spawn_blocking`.
    ///
    /// For each SKILL.md found:
    /// - `skill_name` is the parent directory name
    /// - `tenant_id_str` is the grandparent directory name ("global" or a UUID)
    /// - `path` is the full PathBuf to the SKILL.md file
    pub fn scan_all(&self) -> Vec<SkillFileEntry> {
        let mut entries = Vec::new();

        for entry in walkdir::WalkDir::new(&self.skills_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() != "SKILL.md" {
                continue;
            }

            let path = entry.path().to_path_buf();

            // skill_name = parent directory name
            let skill_name = match path.parent().and_then(|p| p.file_name()) {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            // tenant_id_str = grandparent directory name
            let tenant_id_str = match path
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.file_name())
            {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            entries.push(SkillFileEntry {
                path,
                skill_name,
                tenant_id_str,
            });
        }

        entries
    }

    /// Return the skills root directory for a specific tenant.
    ///
    /// Per D-10: `<skills_root>/<tenant_id>/` is the tenant-specific
    /// directory for private skills.
    pub fn skills_root_for_tenant(&self, tenant_id: TenantId) -> PathBuf {
        self.skills_root.join(tenant_id.to_string())
    }

    /// Return the global skills root directory.
    ///
    /// Per D-10: `<skills_root>/global/` contains built-in skills
    /// available to all tenants.
    pub fn global_skills_root(&self) -> PathBuf {
        self.skills_root.join("global")
    }
}

/// Convert a tenant_id_str (directory name) to a TenantId.
///
/// - "global" maps to `TenantId::nil()` (all-zeros UUID)
/// - Other strings are parsed as UUID -> `TenantId`
pub fn parse_tenant_id_str(tenant_id_str: &str) -> Option<TenantId> {
    if tenant_id_str == "global" {
        Some(TenantId::from(Uuid::nil()))
    } else {
        Uuid::parse_str(tenant_id_str).ok().map(TenantId::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn create_test_skill_structure(root: &Path) {
        // global skill
        let global_dir = root.join("global").join("my-skill");
        fs::create_dir_all(&global_dir).unwrap();
        let mut f = fs::File::create(global_dir.join("SKILL.md")).unwrap();
        writeln!(
            f,
            "---\nname: my-skill\ndescription: A test skill\n---\n# Body\n"
        )
        .unwrap();

        // tenant-specific skill
        let tid = TenantId::new();
        let tenant_dir = root.join(tid.to_string()).join("custom-skill");
        fs::create_dir_all(&tenant_dir).unwrap();
        let mut f = fs::File::create(tenant_dir.join("SKILL.md")).unwrap();
        writeln!(
            f,
            "---\nname: custom-skill\ndescription: Custom\n---\n# Body\n"
        )
        .unwrap();

        // non-SKILL.md file (should be ignored)
        let _ = fs::File::create(global_dir.join("README.md"));
    }

    #[test]
    fn scan_all_finds_all_skill_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("skills");
        create_test_skill_structure(&root);

        let loader = SkillLoader::new(root);
        let entries = loader.scan_all();

        let names: Vec<String> = entries
            .iter()
            .map(|e| e.skill_name.clone())
            .collect();
        assert!(names.contains(&"my-skill".to_string()));
        assert!(names.contains(&"custom-skill".to_string()));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn scan_all_empty_dir_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("skills");
        fs::create_dir_all(&root).unwrap();

        let loader = SkillLoader::new(root);
        let entries = loader.scan_all();
        assert!(entries.is_empty());
    }

    #[test]
    fn skill_file_entry_has_correct_fields() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("skills");
        let skill_dir = root.join("global").join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let mut f = fs::File::create(skill_dir.join("SKILL.md")).unwrap();
        writeln!(f, "---\nname: test-skill\ndescription: T\n---\n\nBody\n").unwrap();

        let loader = SkillLoader::new(root);
        let entries = loader.scan_all();
        assert_eq!(entries.len(), 1);

        let entry = &entries[0];
        assert_eq!(entry.skill_name, "test-skill");
        assert_eq!(entry.tenant_id_str, "global");
        assert!(entry.path.ends_with("SKILL.md"));
    }

    #[test]
    fn skills_root_for_tenant_returns_correct_path() {
        let loader = SkillLoader::new(PathBuf::from("/skills"));
        let tid = TenantId::new();
        let expected = PathBuf::from(format!("/skills/{}", tid));
        assert_eq!(loader.skills_root_for_tenant(tid), expected);
    }

    #[test]
    fn global_skills_root_returns_correct_path() {
        let loader = SkillLoader::new(PathBuf::from("/skills"));
        assert_eq!(loader.global_skills_root(), PathBuf::from("/skills/global"));
    }

    #[test]
    fn parse_tenant_id_str_global_maps_to_nil() {
        let tid = parse_tenant_id_str("global").unwrap();
        assert_eq!(*tid, Uuid::nil());
    }

    #[test]
    fn parse_tenant_id_str_parses_uuid() {
        let uuid = Uuid::now_v7();
        let tid = parse_tenant_id_str(&uuid.to_string()).unwrap();
        assert_eq!(*tid, uuid);
    }

    #[test]
    fn parse_tenant_id_str_invalid_returns_none() {
        assert!(parse_tenant_id_str("not-a-uuid").is_none());
        assert!(parse_tenant_id_str("").is_none());
    }
}