//! Skill lifecycle manager — load, unload, merge, and swap skills.
//!
//! `SkillManager` coordinates the full skill lifecycle: loading SKILL.md files
//! from disk, validating tool availability, injecting into the system prompt,
//! and managing mid-session swaps at turn boundaries.
//!
//! # Design
//!
//! - `load()` reads a SKILL.md from disk, parses it, validates tools, and
//!   inserts into the registry. On failure, the registry is unchanged (D-08).
//! - `unload()` removes a skill and rebuilds the system prompt without it.
//! - `merge_active()` returns the formatted XML context block and merged tool
//!   name list for the current active skills.
//! - `check_pending_swap()` consumes the pending swap flag — called at the top
//!   of each ReAct turn (D-07).

use std::path::PathBuf;
use std::sync::Arc;

use jadepaw_core::{SkillValidationError, TenantId, ToolDefinition, ToolLookup};
use tracing;

use super::injector::build_skill_context_block;
use super::registry::{LoadedSkill, SkillRegistry, SkillSwap};

/// Central coordinator for skill lifecycle management.
///
/// Owns the in-memory registry (shared via `Arc`) and the skills root directory.
/// Designed to be shared as `Arc<SkillManager>` across the agent runtime.
pub struct SkillManager {
    registry: Arc<SkillRegistry>,
    pub skills_root: PathBuf,
}

impl SkillManager {
    /// Create a new SkillManager with the given skills root directory.
    ///
    /// The skills root is typically `~/.jadepaw/skills/`. Individual tenant
    /// skill directories are expected under `<skills_root>/<tenant_id>/<skill_name>/`.
    pub fn new(skills_root: PathBuf) -> Self {
        Self {
            registry: Arc::new(SkillRegistry::new()),
            skills_root,
        }
    }

    /// Load a skill from disk into the in-memory registry.
    ///
    /// # Steps
    ///
    /// 1. Build the file path: `<skills_root>/<tenant_id>/<skill_name>/SKILL.md`
    /// 2. Read the file content via `tokio::fs::read_to_string`
    /// 3. Parse with `parse_skill_file()`
    /// 4. If `tool_lookup` is `Some`, validate each tool in the manifest exists
    ///    in the tool registry. Missing tools return
    ///    `SkillValidationError::ToolNotFound`.
    /// 5. Build a `LoadedSkill` and insert into the registry
    /// 6. Rebuild the system prompt via `merge_active()` and set as pending swap
    ///
    /// # Error handling (D-08)
    ///
    /// If any step fails (parse error, tool not found, IO error), the skill is
    /// NOT inserted into the registry. Existing active skills remain unchanged.
    ///
    /// # Arguments
    ///
    /// * `tenant_id` — the tenant loading this skill
    /// * `skill_name` — the skill directory name (must match manifest name)
    /// * `tool_lookup` — optional tool registry for validating declared tools
    pub async fn load(
        &self,
        tenant_id: TenantId,
        skill_name: &str,
        tool_lookup: Option<&dyn ToolLookup>,
    ) -> Result<(), SkillValidationError> {
        // Step 1: Build file path
        let file_path = self
            .skills_root
            .join(tenant_id.to_string())
            .join(skill_name)
            .join("SKILL.md");

        // Step 2: Read file content
        let content = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
            SkillValidationError::ParseError {
                message: format!("failed to read skill file: {}", e),
                file: file_path.display().to_string(),
                line: None,
            }
        })?;

        // Step 3: Parse
        let (manifest, body) =
            super::parser::parse_skill_file(&content, skill_name, &file_path)?;

        // Step 4: Validate tool availability (D-05)
        if let Some(lookup) = tool_lookup {
            for tool_name in &manifest.tools {
                if lookup.lookup_by_name(tool_name).is_none() {
                    return Err(SkillValidationError::ToolNotFound {
                        tool_name: tool_name.clone(),
                    });
                }
            }
        }

        // Step 5: Build LoadedSkill
        let loaded = LoadedSkill {
            skill_id: jadepaw_core::SkillId::new(),
            manifest,
            body,
            priority: 0,
            loaded_at: chrono::Utc::now(),
        };

        // Step 6: Insert into registry
        self.registry.insert(tenant_id, loaded);

        // Step 7: Rebuild system prompt and set as pending swap
        let (skill_block, _tool_names) = self.merge_active(tenant_id);
        // Build the new system prompt: base_prompt + skill block + tool descriptions.
        // The base prompt and tool descriptions are injected by the caller
        // (run_agent) when constructing the full augmented prompt. Here we
        // store just the skill block and merged tool list for the caller.
        let swap = SkillSwap {
            new_system_prompt: skill_block,
            merged_tool_list: self.merge_tool_definitions(tenant_id, tool_lookup),
        };
        self.registry.set_pending_swap(tenant_id, swap);

        tracing::info!(
            tenant_id = %tenant_id,
            skill = %skill_name,
            "skill loaded successfully"
        );

        Ok(())
    }

    /// Unload a skill from the in-memory registry.
    ///
    /// After removal, rebuilds the system prompt from the remaining skills
    /// and sets a pending swap. If no skills remain, the swap contains an
    /// empty skill block and empty tool list (agent reverts to default).
    ///
    /// # Arguments
    ///
    /// * `tenant_id` — the tenant unloading the skill
    /// * `skill_name` — the name of the skill to remove
    pub async fn unload(
        &self,
        tenant_id: TenantId,
        skill_name: &str,
    ) -> Result<(), SkillValidationError> {
        self.registry.remove(tenant_id, skill_name);

        // Rebuild system prompt from remaining skills
        let (skill_block, _tool_names) = self.merge_active(tenant_id);

        let swap = SkillSwap {
            new_system_prompt: skill_block,
            merged_tool_list: vec![],
        };
        self.registry.set_pending_swap(tenant_id, swap);

        tracing::info!(
            tenant_id = %tenant_id,
            skill = %skill_name,
            "skill unloaded"
        );

        Ok(())
    }

    /// Merge all active skills for a tenant into a formatted context block and
    /// tool name list.
    ///
    /// Returns `(skill_context_block, tool_names)` where:
    /// - `skill_context_block` is the XML `<skill_instructions>` block
    /// - `tool_names` is a deduplicated list of tool names from all active skills
    ///
    /// When no skills are active, returns `("".to_string(), vec![])`.
    pub fn merge_active(&self, tenant_id: TenantId) -> (String, Vec<String>) {
        let active = match self.registry.get_active(tenant_id) {
            Some(skills) if !skills.is_empty() => skills,
            _ => return (String::new(), vec![]),
        };

        let skill_block = build_skill_context_block(&active);

        // Collect tool names — union with dedup
        let mut tool_names: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for skill in &active {
            for tool in &skill.manifest.tools {
                if seen.insert(tool.clone()) {
                    tool_names.push(tool.clone());
                }
            }
        }

        (skill_block, tool_names)
    }

    /// Check and consume the pending skill swap for a tenant.
    ///
    /// Called at the top of each ReAct turn. Returns `Some(SkillSwap)` if a
    /// swap is pending, `None` otherwise. The swap is consumed (take semantics)
    /// per D-07.
    pub fn check_pending_swap(&self, tenant_id: TenantId) -> Option<SkillSwap> {
        self.registry.take_pending_swap(tenant_id)
    }

    /// Check whether a tenant has any active skills.
    pub fn has_active_skills(&self, tenant_id: TenantId) -> bool {
        self.registry.has_active(tenant_id)
    }

    /// List all active skills for a tenant (for API use).
    pub fn list_skills(&self, tenant_id: TenantId) -> Vec<LoadedSkill> {
        self.registry.get_active(tenant_id).unwrap_or_default()
    }

    // ── Private helpers ─────────────────────────────────────────────────────

    /// Build merged tool definitions from active skills and the tool registry.
    ///
    /// For each tool name declared in active skills, looks up the full
    /// `ToolDefinition` from the tool registry. Tools that exist in multiple
    /// skills are deduplicated.
    fn merge_tool_definitions(
        &self,
        tenant_id: TenantId,
        tool_lookup: Option<&dyn ToolLookup>,
    ) -> Vec<ToolDefinition> {
        let lookup = match tool_lookup {
            Some(l) => l,
            None => return vec![],
        };

        let active = match self.registry.get_active(tenant_id) {
            Some(skills) => skills,
            None => return vec![],
        };

        let mut seen = std::collections::HashSet::new();
        let mut definitions = Vec::new();

        for skill in &active {
            for tool_name in &skill.manifest.tools {
                if seen.insert(tool_name.clone()) {
                    if let Some((_id, def)) = lookup.lookup_by_name(tool_name) {
                        definitions.push(def);
                    }
                }
            }
        }

        definitions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillId;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A simple in-memory tool lookup for testing.
    struct TestToolLookup {
        tools: Mutex<HashMap<String, (jadepaw_core::ToolId, ToolDefinition)>>,
    }

    impl TestToolLookup {
        fn new() -> Self {
            Self {
                tools: Mutex::new(HashMap::new()),
            }
        }

        fn register(&self, name: &str) {
            let mut map = self.tools.lock().unwrap();
            map.insert(
                name.to_string(),
                (
                    jadepaw_core::ToolId::new(),
                    ToolDefinition {
                        name: name.to_string(),
                        description: format!("Tool {}", name),
                        input_schema: serde_json::json!({"type": "object"}),
                    },
                ),
            );
        }
    }

    impl ToolLookup for TestToolLookup {
        fn lookup_by_name(
            &self,
            name: &str,
        ) -> Option<(jadepaw_core::ToolId, ToolDefinition)> {
            self.tools.lock().unwrap().get(name).cloned()
        }
    }

    fn tenant() -> TenantId {
        TenantId::new()
    }

    #[test]
    fn new_creates_empty_manager() {
        let mgr = SkillManager::new(PathBuf::from("/tmp/skills"));
        let t = tenant();
        assert!(!mgr.has_active_skills(t));
        let (block, names) = mgr.merge_active(t);
        assert!(block.is_empty());
        assert!(names.is_empty());
    }

    #[test]
    fn merge_active_returns_correct_tool_names() {
        let reg = SkillRegistry::new();
        let t = tenant();

        let mut s1 = LoadedSkill {
            skill_id: SkillId::new(),
            manifest: jadepaw_core::SkillManifest {
                name: "skill-a".to_string(),
                description: "A".to_string(),
                tools: vec!["read_file".to_string(), "write_file".to_string()],
                constraints: None,
                version: None,
                author: None,
                metadata: None,
                source_path: PathBuf::from("/test/SKILL.md"),
            },
            body: "body".to_string(),
            priority: 0,
            loaded_at: chrono::Utc::now(),
        };

        let mut s2 = LoadedSkill {
            skill_id: SkillId::new(),
            manifest: jadepaw_core::SkillManifest {
                name: "skill-b".to_string(),
                description: "B".to_string(),
                tools: vec!["read_file".to_string(), "http_request".to_string()],
                constraints: None,
                version: None,
                author: None,
                metadata: None,
                source_path: PathBuf::from("/test/SKILL.md"),
            },
            body: "body".to_string(),
            priority: 0,
            loaded_at: chrono::Utc::now(),
        };

        reg.insert(t, s1);
        reg.insert(t, s2);

        let mgr = SkillManager {
            registry: Arc::new(reg),
            skills_root: PathBuf::from("/tmp/skills"),
        };

        let (block, names) = mgr.merge_active(t);
        assert!(block.contains("skill-a"));
        assert!(block.contains("skill-b"));
        // Tool names deduplicated: read_file appears in both, should appear once
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"write_file".to_string()));
        assert!(names.contains(&"http_request".to_string()));
    }

    #[test]
    fn check_pending_swap_has_take_semantics() {
        let reg = SkillRegistry::new();
        let t = tenant();
        let swap = SkillSwap {
            new_system_prompt: "test".to_string(),
            merged_tool_list: vec![],
        };
        reg.set_pending_swap(t, swap);

        let mgr = SkillManager {
            registry: Arc::new(reg),
            skills_root: PathBuf::from("/tmp/skills"),
        };

        assert!(mgr.check_pending_swap(t).is_some());
        assert!(mgr.check_pending_swap(t).is_none());
    }

    #[test]
    fn list_skills_returns_empty_for_unknown_tenant() {
        let mgr = SkillManager::new(PathBuf::from("/tmp/skills"));
        let skills = mgr.list_skills(tenant());
        assert!(skills.is_empty());
    }
}