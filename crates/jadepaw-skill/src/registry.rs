//! In-memory skill registry with per-tenant concurrent access.
//!
//! `SkillRegistry` stores loaded skills keyed by `TenantId` using a `DashMap`
//! for lock-free concurrent reads. Each tenant entry holds the active skill
//! state and an optional pending swap (atomic mid-session skill change).
//!
//! # Design
//!
//! - Struct owns `DashMap` directly (not behind `Arc` — callers wrap in
//!   `Arc<SkillRegistry>`).
//! - `ActiveSkillState` holds the loaded skills and the pending swap flag.
//! - `SkillSwap` is the pre-built augmented system prompt and merged tool list
//!   that replaces `messages[0]` at the turn boundary (D-07).
//! - All methods use `DashMap`'s lock-free operations. No `Mutex` or `RwLock`
//!   needed.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use jadepaw_core::{SkillId, SkillManifest, TenantId, ToolDefinition};

/// In-memory registry of loaded skills, keyed by tenant.
///
/// Thread-safe via `DashMap`. Designed to be shared as `Arc<SkillRegistry>`.
pub struct SkillRegistry {
    states: DashMap<TenantId, ActiveSkillState>,
}

impl SkillRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            states: DashMap::new(),
        }
    }

    /// Get the active skills for a tenant, sorted by priority descending.
    ///
    /// Returns a clone of the skills vec so callers can read without holding
    /// a lock. Returns `None` if the tenant has no entry.
    pub fn get_active(&self, tenant_id: TenantId) -> Option<Vec<LoadedSkill>> {
        let state = self.states.get(&tenant_id)?;
        let mut skills = state.loaded_skills.clone();
        skills.sort_by(|a, b| b.priority.cmp(&a.priority));
        Some(skills)
    }

    /// Check whether a tenant has any active skills loaded.
    pub fn has_active(&self, tenant_id: TenantId) -> bool {
        self.states
            .get(&tenant_id)
            .map(|s| !s.loaded_skills.is_empty())
            .unwrap_or(false)
    }

    /// Insert a loaded skill into a tenant's active skill set.
    ///
    /// Creates a new `ActiveSkillState` entry if the tenant does not yet have
    /// one. Skills with the same name are NOT deduplicated here — dedup is
    /// the caller's responsibility (SkillManager::load checks before insert).
    pub fn insert(&self, tenant_id: TenantId, skill: LoadedSkill) {
        let mut state = self.states.entry(tenant_id).or_default();
        state.loaded_skills.push(skill);
    }

    /// Remove a skill from a tenant's active set by name.
    ///
    /// Returns `true` if a skill was removed, `false` if no match was found.
    pub fn remove(&self, tenant_id: TenantId, skill_name: &str) -> bool {
        if let Some(mut state) = self.states.get_mut(&tenant_id) {
            let len_before = state.loaded_skills.len();
            state
                .loaded_skills
                .retain(|s| s.manifest.name != skill_name);
            state.loaded_skills.len() < len_before
        } else {
            false
        }
    }

    /// Atomically set a pending skill swap for a tenant.
    ///
    /// The swap will be consumed by `take_pending_swap()` at the next turn
    /// boundary in the ReAct loop (D-07).
    pub fn set_pending_swap(&self, tenant_id: TenantId, swap: SkillSwap) {
        let mut state = self.states.entry(tenant_id).or_default();
        state.pending_swap = Some(swap);
    }

    /// Atomically take and clear the pending swap for a tenant.
    ///
    /// Returns `Some(SkillSwap)` if a swap was pending, `None` otherwise.
    /// The swap is consumed — subsequent calls return `None` until a new
    /// swap is set (take semantics per D-07).
    pub fn take_pending_swap(&self, tenant_id: TenantId) -> Option<SkillSwap> {
        if let Some(mut state) = self.states.get_mut(&tenant_id) {
            state.pending_swap.take()
        } else {
            None
        }
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// The active skill state for a single tenant.
#[derive(Debug, Clone)]
pub struct ActiveSkillState {
    /// Currently loaded skills for this tenant.
    pub loaded_skills: Vec<LoadedSkill>,
    /// A pending mid-session skill swap, if one was requested.
    pub pending_swap: Option<SkillSwap>,
}

impl Default for ActiveSkillState {
    fn default() -> Self {
        Self {
            loaded_skills: Vec::new(),
            pending_swap: None,
        }
    }
}

/// A single loaded skill in the in-memory registry.
#[derive(Debug, Clone)]
pub struct LoadedSkill {
    /// Unique identifier assigned at load time.
    pub skill_id: SkillId,
    /// The parsed and validated manifest.
    pub manifest: SkillManifest,
    /// The raw Markdown instruction body from the SKILL.md file.
    pub body: String,
    /// Priority for conflict resolution (higher = more precedence).
    /// Default 0. Controlled by metadata `priority` field in the YAML frontmatter.
    pub priority: u8,
    /// When this skill was loaded into the registry.
    pub loaded_at: DateTime<Utc>,
}

/// A pre-built skill swap awaiting application at the next turn boundary.
///
/// Contains the fully constructed system prompt (base prompt + skill context
/// block + tool descriptions) and the merged tool list. Created by
/// `SkillManager` after a load/unload operation and consumed by `react_loop`
/// at the top of the next turn (D-07).
#[derive(Debug, Clone)]
pub struct SkillSwap {
    /// The pre-built augmented system prompt that replaces `messages[0]`.
    pub new_system_prompt: String,
    /// The merged list of tools from all active skills.
    pub merged_tool_list: Vec<ToolDefinition>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillId;

    fn make_skill(name: &str) -> LoadedSkill {
        LoadedSkill {
            skill_id: SkillId::new(),
            manifest: SkillManifest {
                name: name.to_string(),
                description: format!("Skill {}", name),
                tools: vec![],
                constraints: None,
                version: None,
                author: None,
                metadata: None,
                source_path: std::path::PathBuf::from("/test/SKILL.md"),
            },
            body: "Test body".to_string(),
            priority: 0,
            loaded_at: Utc::now(),
        }
    }

    fn tenant() -> TenantId {
        TenantId::new()
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = SkillRegistry::new();
        let t = tenant();
        assert!(reg.get_active(t).is_none());
        assert!(!reg.has_active(t));
    }

    #[test]
    fn insert_and_retrieve() {
        let reg = SkillRegistry::new();
        let t = tenant();
        reg.insert(t, make_skill("code-reviewer"));
        assert!(reg.has_active(t));
        let skills = reg.get_active(t).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest.name, "code-reviewer");
    }

    #[test]
    fn remove_by_name() {
        let reg = SkillRegistry::new();
        let t = tenant();
        reg.insert(t, make_skill("skill-a"));
        reg.insert(t, make_skill("skill-b"));
        assert_eq!(reg.get_active(t).unwrap().len(), 2);

        let removed = reg.remove(t, "skill-a");
        assert!(removed);
        let skills = reg.get_active(t).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].manifest.name, "skill-b");

        // Remove non-existent
        assert!(!reg.remove(t, "nonexistent"));
    }

    #[test]
    fn multi_tenant_isolation() {
        let reg = SkillRegistry::new();
        let t1 = tenant();
        let t2 = tenant();
        reg.insert(t1, make_skill("tenant1-skill"));
        reg.insert(t2, make_skill("tenant2-skill"));

        let s1 = reg.get_active(t1).unwrap();
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].manifest.name, "tenant1-skill");

        let s2 = reg.get_active(t2).unwrap();
        assert_eq!(s2.len(), 1);
        assert_eq!(s2[0].manifest.name, "tenant2-skill");
    }

    #[test]
    fn pending_swap_set_and_take() {
        let reg = SkillRegistry::new();
        let t = tenant();
        let swap = SkillSwap {
            new_system_prompt: "Augmented prompt".to_string(),
            merged_tool_list: vec![],
        };
        reg.set_pending_swap(t, swap);

        let taken = reg.take_pending_swap(t);
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().new_system_prompt, "Augmented prompt");

        // Take again — should be None (take semantics)
        assert!(reg.take_pending_swap(t).is_none());
    }

    #[test]
    fn sorted_by_priority_descending() {
        let reg = SkillRegistry::new();
        let t = tenant();

        let mut s1 = make_skill("low");
        s1.priority = 1;
        let mut s2 = make_skill("high");
        s2.priority = 10;
        let mut s3 = make_skill("mid");
        s3.priority = 5;

        reg.insert(t, s1);
        reg.insert(t, s2);
        reg.insert(t, s3);

        let skills = reg.get_active(t).unwrap();
        assert_eq!(skills[0].manifest.name, "high");
        assert_eq!(skills[1].manifest.name, "mid");
        assert_eq!(skills[2].manifest.name, "low");
    }

    #[test]
    fn default_is_empty() {
        let reg = SkillRegistry::default();
        let t = tenant();
        assert!(!reg.has_active(t));
    }
}