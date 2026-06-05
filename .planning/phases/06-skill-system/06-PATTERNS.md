# Phase 6: Skill System - Pattern Map

**Mapped:** 2026-06-05
**Files analyzed:** 16 new/modified files
**Analogs found:** 16 / 16

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `crates/jadepaw-core/src/skill_types.rs` | model | — | `crates/jadepaw-core/src/types.rs` | exact |
| `crates/jadepaw-skill/src/manifest.rs` | model | — | `crates/jadepaw-core/src/agent_types.rs` | role-match |
| `crates/jadepaw-skill/src/parser.rs` | utility | file-I/O | `crates/jadepaw-agent/src/window.rs` | role-match |
| `crates/jadepaw-skill/src/validation.rs` | utility | — | `crates/jadepaw-core/src/error.rs` | role-match |
| `crates/jadepaw-skill/src/manager.rs` | service | request-response | `crates/jadepaw-agent/src/lib.rs` run_agent() | role-match |
| `crates/jadepaw-skill/src/registry.rs` | service | request-response | `crates/jadepaw-agent/src/tool_registry.rs` | exact |
| `crates/jadepaw-skill/src/loader.rs` | utility | file-I/O | `crates/jadepaw-wasm/src/pool.rs` new() | role-match |
| `crates/jadepaw-skill/src/index.rs` | service | CRUD | `crates/jadepaw-db/src/sqlite_repo.rs` | role-match |
| `crates/jadepaw-skill/src/injector.rs` | utility | transform | `crates/jadepaw-agent/src/llm.rs` build_system_prompt_with_tools() | exact |
| `crates/jadepaw-agent/src/lib.rs` (modify) | controller | request-response | itself — add SkillManager parameter | exact |
| `crates/jadepaw-agent/src/llm.rs` (modify) | utility | transform | itself — add build_skill_augmented_prompt() | exact |
| `crates/jadepaw-agent/src/loop.rs` (modify) | controller | event-driven | itself — add pending swap check at turn top | exact |
| `crates/jadepaw-agent/src/tool_registry.rs` (modify) | service | CRUD | itself — register() panic->Result | exact |
| `crates/jadepaw-core/src/agent_types.rs` (modify) | model | — | itself — add skills field to AgentRequest | exact |
| `crates/jadepaw-db/src/` (new skill_repository, models, sqlite_skill_repo) | service | CRUD | `crates/jadepaw-db/src/repository.rs` | exact |
| `crates/jadepaw-server/src/` (new routes/skills.rs) | controller | request-response | `crates/jadepaw-wasm/src/session.rs` SessionState pattern | partial |

## Pattern Assignments

### 1. `crates/jadepaw-core/src/skill_types.rs` (model, new types)

**Analog:** `crates/jadepaw-core/src/types.rs` lines 1-119

**UUID v7 newtype pattern** (lines 12-41, 49-86, 88-119):

Copy this exact structure for `SkillId`:

```rust
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use uuid::Uuid;

/// Unique identifier for a skill.
///
/// Uses UUID v7 (time-ordered) for database index friendliness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillId(Uuid);

impl SkillId {
    /// Create a new skill identifier using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Deref for SkillId {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl fmt::Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for SkillId {
    fn default() -> Self { Self::new() }
}

impl From<Uuid> for SkillId {
    fn from(u: Uuid) -> Self { Self(u) }
}
```

**SkillManifest struct pattern** — Follow `AgentRequest` from `crates/jadepaw-core/src/agent_types.rs` lines 24-49. Derive `Debug, Clone, PartialEq, Eq, Serialize, Deserialize`. No wasmtime dependency.

**SkillValidationError enum** — Follow `JadepawError` from `crates/jadepaw-core/src/error.rs` lines 13-43. Use `#[derive(Debug, Clone, PartialEq, Eq)]`, implement `fmt::Display` + `std::error::Error`. Variants: `MissingField { field: String }`, `InvalidName { name: String, reason: String }`, `FieldTooLong { field: String, max: usize, actual: usize }`, `NameDirectoryMismatch { expected_name: String, actual_name: String }`, `ToolNotFound { tool_name: String }`, `ParseError { message: String, file: String, line: Option<u32> }`.

**Also modify:** `crates/jadepaw-core/src/lib.rs` — add `pub mod skill_types;` and re-export `pub use skill_types::{SkillId, SkillManifest, SkillValidationError};` following lines 21-36.

---

### 2. `crates/jadepaw-skill/src/manifest.rs` (model struct)

**Analog:** `crates/jadepaw-core/src/agent_types.rs` lines 24-49 (AgentRequest struct pattern)

**Imports pattern** (lines 16-18):
```rust
use crate::types::SessionId;  // from agent_types.rs
use serde::{Deserialize, Serialize};
use std::fmt;
```

**Struct pattern** (lines 24-38):
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub constraints: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// The file path this manifest was loaded from (not in YAML — populated at parse time).
    #[serde(skip)]
    pub source_path: std::path::PathBuf,
}
```

---

### 3. `crates/jadepaw-skill/src/parser.rs` (file I/O + YAML parsing)

**Analog for validation-before-deserialization:** `crates/jadepaw-wasm/src/capability/mod.rs` lines 23-125 (check methods on SessionState impl block — stepwise field validation pattern)

**Analog for fallback/edge-case handling:** `crates/jadepaw-agent/src/window.rs` lines 88-207 (`compress_context` with multiple fallback tiers)

**gray_matter parsing pattern** (from RESEARCH.md Code Examples verified against crate docs):

```rust
use gray_matter::{Matter, engine::YAML, Pod};
use std::path::Path;

pub fn parse_skill_file(
    content: &str,
    dir_name: &str,
    file_path: &Path,
) -> Result<(SkillManifest, String), SkillValidationError> {
    let matter = Matter::<YAML>::new();

    // Step 1: Extract frontmatter and body
    let parsed = matter.parse(content);
    let body = parsed.content.clone();

    // Step 2: Validate YAML parsed successfully
    let data = parsed.data.ok_or_else(|| {
        SkillValidationError::MissingFrontmatter {
            file: file_path.display().to_string(),
        }
    })?;

    // Step 3: Validate required fields via Pod access
    let name = data["name"].as_string()
        .ok_or(SkillValidationError::MissingField { field: "name".into() })?;
    let description = data["description"].as_string()
        .ok_or(SkillValidationError::MissingField { field: "description".into() })?;

    // Step 4: Run custom validation
    validate_skill_name(name)?;

    // Step 5: Name-directory match
    if name != dir_name {
        return Err(SkillValidationError::NameDirectoryMismatch { ... });
    }

    // Step 6: Description length
    if description.len() > 1024 {
        return Err(SkillValidationError::FieldTooLong { ... });
    }

    // Step 7: Extract extension fields, construct SkillManifest
    Ok((SkillManifest { ... }, body))
}
```

**Critical pattern:** Parse into gray_matter's `Pod` FIRST for validation, then construct `SkillManifest`. Do NOT use `parse_with_struct::<SkillManifest>()` directly — it produces opaque serde errors without field-level validation.

**Error handling context wrapping** — Follow `anyhow::Context` pattern from `crates/jadepaw-agent/src/llm.rs` lines 164-168:
```rust
.context("failed to build chat completion request")?;
```

---

### 4. `crates/jadepaw-skill/src/validation.rs` (field validation logic)

**Analog:** `crates/jadepaw-wasm/src/capability/mod.rs` lines 23-53 (check methods that return `bool`, with helper functions for pattern matching)

**validate_skill_name pattern** — named validation functions returning `Result<(), SkillValidationError>`:

```rust
/// Agent Skills spec name validation:
/// - 1-64 characters
/// - lowercase letters, numbers, hyphens only
/// - must not start or end with hyphen
/// - must not contain consecutive hyphens
pub fn validate_skill_name(name: &str) -> Result<(), SkillValidationError> {
    if name.is_empty() || name.len() > 64 {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must be 1-64 characters".into(),
        });
    }
    if !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name may only contain lowercase letters, numbers, and hyphens".into(),
        });
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must not start or end with a hyphen".into(),
        });
    }
    if name.contains("--") {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must not contain consecutive hyphens".into(),
        });
    }
    Ok(())
}
```

---

### 5. `crates/jadepaw-skill/src/registry.rs` (Arc<DashMap<TenantId, SkillState>>)

**Analog:** `crates/jadepaw-agent/src/tool_registry.rs` lines 33-65 (DashMap-based registry)

**Struct pattern** (lines 33-38):
```rust
use dashmap::DashMap;
use jadepaw_core::{SkillId, TenantId};
use std::sync::Arc;

pub struct SkillRegistry {
    /// Tenant -> active skill state mapping.
    states: Arc<DashMap<TenantId, ActiveSkillState>>,
}

pub struct ActiveSkillState {
    pub loaded_skills: Vec<LoadedSkill>,
    pub pending_swap: Option<SkillSwap>,
}

pub struct LoadedSkill {
    pub skill_id: SkillId,
    pub manifest: super::manifest::SkillManifest,
    pub body: String,
    pub priority: u8,
    pub loaded_at: chrono::DateTime<chrono::Utc>,
}
```

**`new()` pattern** (lines 42-47):
```rust
impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            states: Arc::new(DashMap::new()),
        }
    }
}
```

**Concurrency pattern:** Read-heavy (every turn reads active skills), write-rare (only on API load/unload). DashMap provides lock-free reads. Only `DashMap` itself is wrapped in `Arc`, NOT the struct (follows ToolRegistry pattern where struct owns DashMap directly, callers wrap in `Arc<SkillRegistry>`).

---

### 6. `crates/jadepaw-skill/src/manager.rs` (SkillManager coordinator)

**Analog for coordinator/builder struct:** `crates/jadepaw-agent/src/lib.rs` lines 42-171 (`run_agent()` — orchestrates multiple subsystems)

**Analog for concurrent state dispatch:** `crates/jadepaw-agent/src/tool_registry.rs` lines 107-175 (`call_tool()` — 3-step dispatch: lookup -> check -> execute)

**Struct pattern:**
```rust
pub struct SkillManager {
    pub registry: Arc<SkillRegistry>,
    pub loader: SkillLoader,
    pub index: SkillIndex,
}

impl SkillManager {
    /// Load a skill for a tenant: parse file, validate, register in DashMap, set pending swap.
    pub async fn load(&self, tenant_id: TenantId, skill_name: &str) -> Result<(), SkillValidationError> { ... }

    /// Unload a skill: remove from DashMap, set pending swap.
    pub async fn unload(&self, tenant_id: TenantId, skill_name: &str) -> Result<(), SkillValidationError> { ... }

    /// Merge all active skills into a system prompt block + tool list.
    pub fn merge_active(&self, tenant_id: TenantId) -> (String, Vec<ToolDefinition>) { ... }

    /// Check for pending skill swap at turn boundary.
    pub fn check_pending_swap(&self, tenant_id: TenantId) -> Option<SkillSwap> { ... }
}
```

**Method signatures follow `SessionRepository` trait pattern** from `crates/jadepaw-db/src/repository.rs` lines 38-83 — every method with tenant_id parameter for dual-key isolation.

---

### 7. `crates/jadepaw-skill/src/loader.rs` (file I/O, walkdir scanning)

**Analog for startup I/O:** `crates/jadepaw-db/src/sqlite_repo.rs` lines 47-68 (`SqliteSessionRepo::new()` — construction with I/O, returns Result)

**Analog for scan + process pattern:** `crates/jadepaw-agent/src/window.rs` lines 265-340 (`build_summary()` — iterate over items, extract key info)

**walkdir scanning — MUST wrap in `tokio::task::spawn_blocking`.** This is the primary anti-pattern to avoid (Pitfall 4 from RESEARCH.md).

```rust
pub struct SkillLoader {
    skills_root: PathBuf,
}

impl SkillLoader {
    /// Scan all SKILL.md files under the skills root.
    /// Runs at server startup. Called via spawn_blocking.
    pub fn scan_all(&self) -> Vec<SkillFileEntry> {
        let mut entries = Vec::new();
        for entry in walkdir::WalkDir::new(&self.skills_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "SKILL.md")
        {
            // Extract tenant_id and skill_name from path components
            // skills_root/<tenant_id>/<skill_name>/SKILL.md
            ...
            entries.push(SkillFileEntry { path, skill_name, tenant_id });
        }
        entries
    }
}
```

**Startup call site** — Follow `SqliteSessionRepo::new()` async construction pattern (lines 47-68):
```rust
pub async fn new(skills_root: PathBuf, index: Arc<SkillIndex>) -> Result<Self> {
    let loader = Self { skills_root, index };
    // Run scan in blocking task
    let entries = tokio::task::spawn_blocking(move || loader.scan_all())
        .await
        .context("skill scan panicked")??;
    // Then sync to SQLite index
    ...
}
```

---

### 8. `crates/jadepaw-skill/src/index.rs` (SQLite cache operations)

**Analog:** `crates/jadepaw-db/src/sqlite_repo.rs` lines 40-68 (construction pattern), lines 78-129 (save/upsert), lines 216-295 (list_by_tenant)

**Use the same SQLite pool from `SqliteSessionRepo`.** Do not create a separate pool. SkillRepository gets a reference to the shared pool.

```rust
pub struct SkillIndex {
    pool: sqlx::SqlitePool,
}

impl SkillIndex {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    /// Bulk upsert skill metadata into the index table.
    pub async fn sync(&self, entries: &[SkillFileEntry]) -> Result<()> { ... }

    /// List all indexed skills for a tenant.
    pub async fn list_by_tenant(&self, tenant_id: TenantId) -> Result<Vec<SkillIndexSummary>> { ... }

    /// Look up a single skill by name and tenant.
    pub async fn get_by_name(&self, tenant_id: TenantId, name: &str) -> Result<Option<SkillIndexRecord>> { ... }
}
```

**Query pattern** — Follow `crates/jadepaw-db/src/sqlite_repo.rs`:
- Lines 89-116: raw SQL string queries (NOT sqlx::query! macro — this codebase uses raw `sqlx::query()` with `.bind()`)
- Lines 104: UUID binding as `.bind(session_id.as_bytes().as_slice())`
- Lines 166: UUID extraction via `Uuid::from_slice(&raw).context("invalid ... BLOB")?`
- Lines 122-128: `result.rows_affected() == 0` check for cross-tenant collision detection
- Lines 265-268: `chrono::DateTime::parse_from_rfc3339(&str)...with_timezone(&chrono::Utc)`

---

### 9. `crates/jadepaw-skill/src/injector.rs` (system prompt XML block builder)

**Analog:** `crates/jadepaw-agent/src/llm.rs` lines 108-137 (`build_system_prompt_with_tools()` — takes base prompt + data, returns formatted String)

**Pattern** (lines 116-132):
```rust
pub fn build_skill_context_block(
    active_skills: &[LoadedSkill],
) -> String {
    let mut sorted = active_skills.to_vec();
    sorted.sort_by_key(|s| std::cmp::Reverse(s.priority));

    let mut block = String::from("<skill_instructions>\n");
    for skill in &sorted {
        block.push_str(&format!(
            "<skill name=\"{}\" version=\"{}\" priority=\"{}\">\n",
            skill.manifest.name,
            skill.manifest.version.as_deref().unwrap_or("unknown"),
            skill.priority,
        ));
        block.push_str(&skill.body);
        block.push_str("\n</skill>\n");
    }
    block.push_str("</skill_instructions>");
    block
}
```

**Integration pattern** — mirrors `build_system_prompt_with_tools` call pattern from `crates/jadepaw-agent/src/lib.rs` lines 103-108:
```rust
let augmented_prompt = if skill_manager.has_active_skills(tenant_id) {
    let (skill_block, _) = skill_manager.merge_active(tenant_id);
    llm::build_skill_augmented_prompt(system_prompt, &skill_block, &tool_definitions)
} else if tool_definitions.is_empty() {
    system_prompt.to_string()
} else {
    llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
};
```

---

### 10. `crates/jadepaw-agent/src/lib.rs` (modify — add SkillManager parameter)

**Self-analog:** lines 66-171 (`run_agent()`)

**Changes needed:**
- Line 72: Add `skill_manager: Option<Arc<SkillManager>>` parameter after `tool_registry` (follows `Option<Arc<T>>` pattern from line 72)
- Line 99-108: Replace static system prompt building with skill-aware version:
  ```rust
  let system_prompt = llm::REACT_SYSTEM_PROMPT;
  let tool_definitions = registry.list_tools();
  let augmented_prompt = if let Some(ref sm) = skill_manager {
      if sm.has_active_skills(tenant_id) {
          let (skill_block, _) = sm.merge_active(tenant_id);
          llm::build_skill_augmented_prompt(system_prompt, &skill_block, &tool_definitions)
      } else if tool_definitions.is_empty() {
          system_prompt.to_string()
      } else {
          llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
      }
  } else if tool_definitions.is_empty() {
      system_prompt.to_string()
  } else {
      llm::build_system_prompt_with_tools(system_prompt, &tool_definitions)
  };
  ```
- Line 114: Pass `skill_manager` into `react_loop()` call
- Same changes for `resume_session()` (lines 186-371)

**Parameter threading pattern:** Follow existing pattern — `tool_registry: Option<Arc<ToolRegistry>>` line 72, `session_repo: Option<&dyn SessionRepository>` line 140.

---

### 11. `crates/jadepaw-agent/src/llm.rs` (modify — add build_skill_augmented_prompt)

**Self-analog:** lines 108-137 (`build_system_prompt_with_tools()`)

**New function — same signature pattern:**

```rust
/// Augment the system prompt with skill instructions AND tool descriptions.
///
/// Skill context is injected as an XML block before tool descriptions.
/// Tools are injected in MCP tools/list format.
pub fn build_skill_augmented_prompt(
    base_prompt: &str,
    skill_context_block: &str,
    tools: &[ToolDefinition],
) -> String {
    // ... same tool formatting as build_system_prompt_with_tools
    format!(
        "{}\n\n{}\n\nAvailable tools:\n{}\n\nWhen calling a tool, use the exact tool name and provide parameters as a JSON object.",
        base_prompt,
        skill_context_block,
        tool_descriptions.join("\n")
    )
}
```

**Error handling pattern** (lines 123-127): Use `tracing::warn!` for soft failures, `unwrap_or_else` for fallback values. Do NOT panic.

---

### 12. `crates/jadepaw-agent/src/loop.rs` (modify — pending swap at turn top)

**Self-analog:** lines 129-372 (`react_loop()`)

**Insert at top of for-turn loop** (after line 161, before fuel reset):

```rust
// Check for pending skill swap (D-07)
if let Some(skill_swap) = skill_manager
    .as_ref()
    .and_then(|sm| sm.check_pending_swap(tenant_id))
{
    // Replace messages[0] (system prompt) atomically
    let system_msg: ChatCompletionRequestMessage =
        ChatCompletionRequestSystemMessage::from(skill_swap.new_system_prompt).into();
    messages[0] = system_msg;

    // Rebuild tool list from merged declarations
    // SkillManager::merge_active() produces the merged tool list alongside
    // the augmented prompt, which is already captured above.
    tracing::info!(
        session_id = %session_id,
        "applied pending skill swap at turn {}",
        turn
    );
}
```

**Timing:** This check runs at the TOP of each turn, AFTER context compression (line 176-186), BEFORE `stream_llm_response()` (line 189). Never during an in-flight LLM call (per D-07).

**Function signature change** (line 129): Add `skill_manager: Option<Arc<SkillManager>>` parameter after `tool_registry`.

**messages[0] preservation** — Already guaranteed by `compress_context` in `crates/jadepaw-agent/src/window.rs` lines 143-148:
```rust
if messages.len() >= 2 {
    result.push(messages[0].clone()); // system prompt preserved
    result.push(messages[1].clone());
}
```

---

### 13. `crates/jadepaw-agent/src/tool_registry.rs` (modify — register() panic to Result)

**Self-analog:** lines 54-65 (`register()`)

**Current (panic):**
```rust
pub fn register(&self, tool: Arc<dyn Tool>) -> ToolId {
    let id = ToolId::new();
    let name = tool.name().to_string();
    if self.name_index.contains_key(&name) {
        panic!("ToolRegistry: duplicate tool name '{}'", name);
    }
    self.name_index.insert(name, id);
    self.tools.insert(id, tool);
    id
}
```

**New (Result):**
```rust
#[derive(Debug, thiserror::Error)]
pub enum ToolConflictError {
    #[error("duplicate tool name '{name}': existing priority {existing}, new priority {new}")]
    DuplicateTool { name: String, existing: u8, new: u8 },
    #[error("tool '{name}' already registered with incompatible schema")]
    SchemaMismatch { name: String },
}

pub fn register(&self, tool: Arc<dyn Tool>, priority: u8) -> Result<ToolId, ToolConflictError> {
    let id = ToolId::new();
    let name = tool.name().to_string();
    if let Some(_existing_id) = self.name_index.get(&name) {
        // D-04: Union merge — higher priority wins, same name/different schema warns
        return Err(ToolConflictError::DuplicateTool {
            name,
            existing: 0, // to be determined from stored priority
            new: priority,
        });
    }
    self.name_index.insert(name, id);
    self.tools.insert(id, tool);
    Ok(id)
}
```

**Error enum pattern** — Follow `LoopErrorKind` from `crates/jadepaw-agent/src/loop.rs` lines 39-67: derive `Debug`, `thiserror::Error` via `#[error("...")]`, implement `fmt::Display` through the derive.

**Note:** Adding `priority` to `register()` requires adding a `priority: u8` field to the internal tool storage and a `register_with_priority` helper. Existing callers in tests must be updated from `registry.register(Arc::new(...))` to `registry.register(Arc::new(...), 0).unwrap()`.

---

### 14. `crates/jadepaw-core/src/agent_types.rs` (modify — add skills field)

**Self-analog:** lines 24-38 (`AgentRequest` struct)

**Add field** (after line 32 `pub context: Option<String>,`):
```rust
/// Skills to activate for this session.
///
/// When set, the SkillManager loads these skills and injects
/// their instructions into the system prompt. Skills are
/// identified by name (kebab-case), matching the directory name
/// under ~/.jadepaw/skills/<tenant_id>/.
#[serde(default)]
pub skills: Vec<String>,
```

**Also add to `Default` impl** (line 44):
```rust
skills: Vec::new(),
```

**Backward compatibility:** `#[serde(default)]` ensures existing JSON without `skills` field deserializes correctly (empty vec). The `context` field is preserved for backward compat — skill injection is additive.

---

### 15. `crates/jadepaw-db/src/` (new skill_repository, models, sqlite_skill_repo, migration)

**Analog:** `crates/jadepaw-db/src/repository.rs` (trait), `models.rs` (data models), `sqlite_repo.rs` (impl), `migrations.rs` (docs)

#### 15a. `crates/jadepaw-db/src/skill_repository.rs` (trait)

Copy pattern from `crates/jadepaw-db/src/repository.rs` lines 1-95:

```rust
//! Skill repository trait.
//!
//! All methods require both `skill_id` and `tenant_id` as mandatory
//! parameters -- the type system enforces isolation at every call site (D-08).

use async_trait::async_trait;
use anyhow::Result;
use jadepaw_core::{SkillId, TenantId};
use crate::skill_models::{SkillIndexRecord, SkillIndexSummary};

#[async_trait]
pub trait SkillRepository: Send + Sync {
    /// Bulk upsert skill metadata into the cache index.
    async fn sync_index(&self, entries: &[SkillIndexRecord]) -> Result<()>;

    /// List all indexed skills for a tenant.
    async fn list_by_tenant(&self, tenant_id: TenantId) -> Result<Vec<SkillIndexSummary>>;

    /// Look up a skill by name and tenant.
    async fn get_by_name(&self, tenant_id: TenantId, name: &str) -> Result<Option<SkillIndexRecord>>;

    /// Remove a skill from the index.
    async fn delete(&self, skill_id: SkillId, tenant_id: TenantId) -> Result<()>;
}
```

#### 15b. `crates/jadepaw-db/src/skill_models.rs` (data models)

Copy pattern from `crates/jadepaw-db/src/models.rs` lines 1-104:

```rust
use jadepaw_core::{SkillId, TenantId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillIndexRecord {
    pub skill_id: SkillId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub tools_json: String,         // JSON array of tool names
    pub file_path: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillIndexSummary {
    pub skill_id: SkillId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: String,
    pub version: Option<String>,
}
```

#### 15c. `crates/jadepaw-db/src/sqlite_skill_repo.rs` (SQLite impl)

Copy pattern from `crates/jadepaw-db/src/sqlite_repo.rs`:

- Lines 40-68: `new()` — same pool pattern, `SqliteConnectOptions`, WAL mode, `busy_timeout`, `create_if_missing`, `max_connections(5)`. IMPORTANT: `SqliteSkillRepo` does NOT create its own pool. It receives a `SqlitePool` reference/Arc from the shared pool.
- Lines 89-116: `save()` / `sync_index()` — INSERT OR REPLACE pattern
- Lines 142-158: `load()` / `get_by_name()` — `WHERE ... AND tenant_id = ?` dual-key pattern with `fetch_optional`
- Lines 216-294: `list_by_tenant()` — `ORDER BY created_at DESC` pattern
- Lines 302-308: `delete()` — idempotent DELETE pattern
- UUID BLOB pattern: `.bind(skill_id.as_bytes().as_slice())` for binding, `Uuid::from_slice(&raw).context("...")` for extraction

#### 15d. Migration file

Create `crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql`:

```sql
-- Create skill_index table for Phase 6 skill metadata cache.
-- Acts as a fast lookup index; source of truth is the filesystem SKILL.md files.

CREATE TABLE IF NOT EXISTS skill_index (
    skill_id        BLOB PRIMARY KEY NOT NULL,
    tenant_id       BLOB NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    version         TEXT,
    tools_json      TEXT NOT NULL DEFAULT '[]',
    file_path       TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_skill_index_tenant_name
    ON skill_index (tenant_id, name);

CREATE INDEX IF NOT EXISTS idx_skill_index_tenant_created
    ON skill_index (tenant_id, created_at);
```

**Migration naming:** Follow `20260604000001_create_sessions.sql` pattern — `YYYYMMDDHHMMSS_description.sql`.

#### 15e. `crates/jadepaw-db/src/lib.rs` (modify)

Add new modules following lines 20-27:
```rust
pub mod skill_models;
pub mod skill_repository;
pub mod sqlite_skill_repo;

pub use skill_models::{SkillIndexRecord, SkillIndexSummary};
pub use skill_repository::SkillRepository;
pub use sqlite_skill_repo::SqliteSkillRepo;
```

---

### 16. `crates/jadepaw-server/src/` (REST routes for skills)

**Analog for route structure:** No existing route files exist (main.rs is a stub). Follow `axum` API style consistent with codebase use in:
- `crates/jadepaw-agent/src/lib.rs` lines 34-38 (axum re-exports via `axum::response::sse::Event`)
- `crates/jadepaw-agent/src/stream.rs` for channel pattern

**Route handler pattern:**

```rust
// crates/jadepaw-server/src/routes/skills.rs

use axum::{Router, Json, extract::{Path, Query, State}, routing::{get, post}, http::StatusCode};
use jadepaw_core::{TenantId};
use jadepaw_skill::SkillManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone)]
pub struct SkillApiState {
    pub skill_manager: Arc<SkillManager>,
}

#[derive(Deserialize)]
pub struct LoadSkillRequest {
    pub tenant_id: TenantId,
    pub skill_name: String,
}

#[derive(Deserialize)]
pub struct ListSkillsQuery {
    pub tenant_id: TenantId,
}

#[derive(Serialize)]
pub struct SkillInspectResponse {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tools: Vec<String>,
    pub constraints: Option<String>,
    pub body: String,
}

#[derive(Serialize)]
pub struct SkillValidationErrorResponse {
    pub field: String,
    pub reason: String,
}

pub fn skill_routes() -> Router<SkillApiState> {
    Router::new()
        .route("/skills/load", post(load_skill))
        .route("/skills/unload", post(unload_skill))
        .route("/skills/list", get(list_skills))
        .route("/skills/inspect/{name}", get(inspect_skill))
}

async fn load_skill(
    State(state): State<SkillApiState>,
    Json(req): Json<LoadSkillRequest>,
) -> Result<StatusCode, (StatusCode, Json<SkillValidationErrorResponse>)> {
    state.skill_manager.load(req.tenant_id, &req.skill_name).await
        .map(|_| StatusCode::OK)
        .map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(SkillValidationErrorResponse {
                field: e.field,
                reason: e.reason,
            }))
        })
}

// ... similar for unload, list, inspect
```

**State extraction pattern** — Follow `axum::extract::State` usage (standard axum 0.8 pattern). Use `#[derive(Clone)]` on state struct per axum requirements.

---

## Shared Patterns

### Authentication
**Source:** Not yet implemented in codebase. Phase 6 routes use no auth (deferred to Phase 9). All skill operations gate on `tenant_id` parameter for isolation (D-10, D-11).

### Error Handling
**Source:** `crates/jadepaw-core/src/error.rs` lines 1-109

All new error types follow the `JadepawError` pattern:
- Derive `Debug, Clone, PartialEq, Eq`
- Implement `fmt::Display` manually (match arms with `write!`)
- Implement `std::error::Error` with `fn source()` returning `None` for self-contained variants
- Constructor methods: `pub fn capability_denied(...)` -> `Self`
- Error methods return `-> Result<T, JadepawError>` on the public API, `anyhow::Result<T>` for internal use (see `loop.rs`)

### UUID v7 Newtype
**Source:** `crates/jadepaw-core/src/types.rs` lines 12-86

Every ID type follows the exact pattern (SkillId being added in Phase 6):
1. `pub struct SkillId(Uuid)` with derive macros: `Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize`
2. `impl SkillId { pub fn new() -> Self { Self(Uuid::now_v7()) } }`
3. `impl Deref for SkillId { type Target = Uuid; ... }` — enables `.as_bytes()`
4. `impl fmt::Display` — delegates to `self.0`
5. `impl Default` — calls `Self::new()`
6. `impl From<Uuid>` — provides `SessionId::from(uuid)`

### DashMap Concurrency
**Source:** `crates/jadepaw-agent/src/tool_registry.rs` lines 33-38, `crates/jadepaw-wasm/src/pool.rs` lines 123-136

Pattern: Struct owns `DashMap` directly (not behind Arc). Callers wrap in `Arc<StructName>`. Key = `TenantId` (or `SessionId` for single-key). Value = state struct. DashMap provides lock-free reads — no Mutex/RwLock needed.

### SQLite Repository
**Source:** `crates/jadepaw-db/src/sqlite_repo.rs` lines 31-68, 78-129, 132-208

Pattern checklist:
- Pool owned directly in struct (not behind Arc)
- `SqliteConnectOptions` with WAL, busy_timeout, foreign_keys, create_if_missing
- `max_connections(5)` for SQLite
- `sqlx::migrate!("./migrations")` at construction
- UUID BLOB binding: `.bind(id.as_bytes().as_slice())`
- UUID BLOB extraction: `Uuid::from_slice(&raw).context("invalid ... BLOB")?`
- Timestamp storage: RFC3339 strings via `chrono::DateTime::to_rfc3339()`
- Dual-key isolation: `WHERE session_id = ? AND tenant_id = ?` on every query
- Raw `sqlx::query()` calls (not `sqlx::query!()` macro — the codebase uses string SQL with manual binding)
- `rows_affected() == 0` cross-tenant collision check on upsert

### System Prompt Building
**Source:** `crates/jadepaw-agent/src/llm.rs` lines 108-137

Pattern: Pure function with `&str` inputs, returns `String`. Uses `format!()` for template construction. Calls `serde_json::to_string()` with `unwrap_or_else` fallback + `tracing::warn!` for serialization failures. No side effects.

### ReAct Loop Turn Structure
**Source:** `crates/jadepaw-agent/src/loop.rs` lines 161-371

Turn boundary order within the for-loop:
1. Fuel reset (line 164)
2. Context window check + compression (line 176)
3. **<-- Skill swap check GOES HERE (before LLM call)**
4. LLM streaming call (line 189)
5. Thought emit + parse (lines 204-216)
6. Action dispatch -> Observation -> message append (lines 218-308)
7. Persist checkpoint (line 316)

---

## No Analog Found

All files have adequate analogs in the existing codebase. The closest case where the analog is partial (not exact) is the server routes (`crates/jadepaw-server/src/routes/skills.rs`) — the server crate's `main.rs` is a stub, so there's no existing route handler to copy. However, the `axum::Router` API is well-documented in the axum 0.8 docs referenced in RESEACH.md, and the `State` extractor pattern is standard.

---

## Metadata

**Analog search scope:** All crates/ subdirectories (jadepaw-core, jadepaw-agent, jadepaw-wasm, jadepaw-db, jadepaw-skill, jadepaw-server)
**Files scanned:** 18 source files + 1 migration file + 2 Cargo.toml files
**Pattern extraction date:** 2026-06-05
**Framework:** Rust 2024 edition, wasmtime 45, tokio 1.52, axum 0.8, dashmap 6, sqlx 0.9
**New workspace dependencies for jadepaw-skill/Cargo.toml:** `gray_matter = { workspace = true }`, `walkdir = { workspace = true }`
**New workspace Cargo.toml entries:** `gray_matter = { version = "0.3", default-features = true }`, `walkdir = "2.5"`