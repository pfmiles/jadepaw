# Phase 6: Skill System - Research

**Researched:** 2026-06-05
**Domain:** Declarative Agent Skill format parsing, hot-loading, system prompt injection, multi-tenant skill discovery
**Confidence:** HIGH

## Summary

Phase 6 implements a declarative skill system where users define agent behaviors through SKILL.md files (YAML frontmatter + Markdown instructions) that can be loaded, swapped, and unloaded at runtime without restarting the agent. The implementation lives primarily in `crates/jadepaw-skill/` (currently a placeholder) and integrates into `crates/jadepaw-agent/` at the system prompt building and ReAct loop turn-boundary points.

The Agent Skills open standard (agentskills.io) defines the SKILL.md format: `---\nYAML frontmatter\n---\nMarkdown body`. Required fields are `name` (kebab-case, 1-64 chars) and `description` (1-1024 chars). jadepaw extends this with `tools`, `constraints`, `version`, and `author` fields (per D-01). The standard validator silently ignores unknown fields, making jadepaw extensions backward-compatible.

The primary technical challenge is integrating skill context injection into the existing ReAct loop without introducing data races during mid-session swaps. The late-binding approach (D-03) rebuilds system prompts at each turn boundary, enabling atomic swaps through a `pending_skill_change` flag pattern. The existing `compress_context()` (Phase 5) already preserves `messages[0]` (system prompt), ensuring skill instructions survive compression.

**Primary recommendation:** Use gray_matter 0.3.2 + yaml-rust2 0.10 for YAML frontmatter parsing (serde_yaml is deprecated), walkdir 2.5.0 for directory scanning, and integrate SkillManager as an Arc-wrapped singleton with DashMap state that follows the existing ToolRegistry concurrency pattern.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SKILL-01 | Declarative Skill format using Agent Skills open standard (SKILL.md: YAML frontmatter + Markdown instructions), versionable | Agent Skills spec (agentskills.io/specification) fully documented in sections below. gray_matter 0.3.2 handles frontmatter extraction. yaml-rust2 provides YAML 1.2 parsing. |
| SKILL-02 | Skill hot loading and runtime swapping, injected as persistent behavioral configuration into agent execution context | Late-binding system prompt rebuild at turn boundaries (D-03/D-07). SkillManager::merge_active() produces merged system prompt block. ToolRegistry union merge with priority (D-04). Pending_skill_change flag pattern for atomic mid-session swaps. |
</phase_requirements>

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Agent Skills 标准 + 精选扩展字段。遵循 agentskills.io 开放标准（`name` + `description` 必填），顶层增加 jadepaw 特有字段：`tools`（结构化工具声明，映射到 ToolRegistry）、`constraints`（自然语言约束，注入 system prompt）、`version`（语义化版本，为 v2 版本管理预留）、`author`（归属）。扩展字段在标准 validator 中被静默忽略，在 jadepaw parser 中做结构化校验。YAML frontmatter 缺失必填字段在解析阶段即被拒绝（符合 success criterion 4）。
- **D-02:** Hybrid Layered Injection（XML tag 结构化包裹）。所有 active skill 的指令注入到 system prompt 中，使用 `<skill_instructions>` XML block 包裹每条 skill（含 name、version、priority 属性）。多 skill 按 priority 排序（默认 0，高优先级覆盖低冲突声明）。提供结构化分隔和可审计性。
- **D-03:** Late-Binding 动态重建。每轮 ReAct turn 开始前从 `SkillManager::merge_active()` 重建 system prompt，替换 `messages[0]`。支持 mid-session skill swap —— 当前 turn 完成后，下一 turn 前原子替换。`compress_context`（Phase 5）保留 `messages[0]` 的逻辑确保压缩后 skill 指令不丢失。
- **D-04:** Union 合并策略。多个 skill 的工具声明取并集，通过 `ToolRegistry`（已有 DashMap 并发安全架构）注册。同一 tool 重名时：priority 高的 skill 版本生效，schema 不同时 warn + skip。`register()` panic-on-duplicate 改为 `Result`。
- **D-05:** Skill 加载时验证工具可用性。声明引用的 `ToolId` 必须在 `ToolRegistry` 中存在 —— 缺失则拒绝加载，返回结构化 `SkillValidationError { field, reason }`。不静默降级。
- **D-06:** 纯 API 驱动。load/unload/reload/list 通过 REST API 操作（aligns with jadepaw Web-first architecture）。无文件监听（notify）或轮询 —— 文件系统仅作为持久化存储层。Phase 7+ 可扩展 Hybrid 模式（API + file watch via jadepaw-bus）。
- **D-07:** Mid-session swap 在当前 turn 完成后生效。检测到 skill change 时设置 `pending_skill_change` flag，在 next turn 顶部原子交换 `messages[0]` 并重建 tool list。不中断 in-flight LLM 调用 —— 避免上下文不一致。
- **D-08:** 验证失败拒绝并保留当前 skill。新 skill 在 staging area 解析/校验，全部通过后原子 swap。正在使用的 skill 不受影响。
- **D-09:** 文件主存储 + SQLite 索引缓存。SKILL.md 存放于 `~/.jadepaw/skills/<tenant_id>/<skill_name>/SKILL.md` 作为 source of truth。启动时 walkdir 扫描全部目录，提取 YAML frontmatter 元数据写入 SQLite `skill_index` 表（名称、描述、版本、tools 列表、文件路径）。API `list_skills` 走 DB 索引查询（O(log n)），`load_skill` 直接读文件（source of truth）。DB 索引可丢弃 —— 删除后重新扫描即重建。
- **D-10:** 多租户目录隔离。`~/.jadepaw/skills/global/` 存放内置技能，`~/.jadepaw/skills/<tenant_id>/` 存放租户私有技能。租户目录优先（同名可覆盖全局）。路径校验复用 Phase 2 的 canonicalize + prefix check 基础设施。
- **D-11:** Skill 运行时状态在内存中管理。`Arc<DashMap<TenantId, SkillState>>` 模式（对齐 InstancePool/ToolRegistry 的并发架构）。重启时从文件系统重新加载。Phase 7+ 集群模式加 DB-backed skill state（`SkillRepository` trait extends SessionRepository pattern）。

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

### Deferred Ideas (OUT OF SCOPE)
- **交互式 Skill 创建 (SKILL-03):** 对话引导 → 意图提取 → 草稿生成 → Wasm 沙箱预览 → 迭代。v2 的 "aha moment" 功能。
- **Skill 版本管理和持续迭代 (SKILL-04):** Git-based 版本历史，diff，回滚。Phase 6 的 `version` 字段为此时预留。
- **Skill 组合为工作流 DAG (SKILL-05):** 多 skill 编排，通过宿主消息总线传递。v2。
- **文件监听自动重载 (notify crate):** Phase 6 用 API 驱动。Hybrid API+Watch 方案（jadepaw-bus 为脊柱）推迟到 Phase 7+。
- **Git-based Skill 分发/市场 (UI-04):** 文件系统设计已兼容 —— `git clone` 到技能目录即可自动索引。完整市场 UI 在 v2。
- **Per-skill token budget:** 每条 skill 的指令占用 token 预算管理。当前 context window 65% 压缩阈值已覆盖全局。
- **Skill 间显式冲突解决 DSL:** XML block 中的 `<conflict resolution>` 语义标记。当前用 priority-based override 覆盖基本需求。
</user_constraints>

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| SKILL.md file parsing (YAML frontmatter + Markdown body) | jadepaw-skill | — | Core parsing logic; owns the Manifest struct and validation |
| Skill filesystem discovery (walkdir scanning) | jadepaw-skill | — | File I/O belongs in the skill crate |
| Skill -> system prompt injection | jadepaw-agent | jadepaw-skill | Agent owns message construction; skill crate produces the formatted text block |
| Tool declaration merging (union with priority) | jadepaw-agent (ToolRegistry) | jadepaw-skill | ToolRegistry owns tool dispatch; skill crate provides the parsed tool list |
| Skill hot-load/unload API endpoints | jadepaw-server (routes) | jadepaw-skill (SkillManager) | Server owns HTTP routing; SkillManager provides the load/unload primitives |
| Skill index caching (SQLite) | jadepaw-db | jadepaw-skill | DB crate owns all persistence; skill crate drives sync operations |
| Mid-session skill swap (turn-boundary atomicity) | jadepaw-agent (react_loop) | jadepaw-skill | Agent loop owns message history; skill crate provides the swap signal |
| Multi-tenant directory isolation | jadepaw-skill | jadepaw-wasm (path validation) | Skill crate builds paths from tenant_id; wasm crate provides canonicalize+prefix check |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| **gray_matter** | 0.3.2 | YAML frontmatter extraction from Markdown strings | Only maintained Rust frontmatter parser. Extracts `---` delimited YAML and body content. Uses yaml-rust2 under the hood. Supports custom struct deserialization. [VERIFIED: crates.io `gray_matter` 0.3.2] |
| **yaml-rust2** | 0.10 (via gray_matter) | YAML 1.2 parsing engine | gray_matter's YAML engine dependency. serde_yaml is deprecated (since March 2024). yaml-rust2 is pure Rust, YAML 1.2 compliant. [VERIFIED: crates.io `yaml-rust2` 0.11.0 available; gray_matter 0.3.2 pins 0.10] |
| **walkdir** | 2.5.0 | Recursive directory scanning | De facto standard for Rust directory traversal. By BurntSushi (same author as ripgrep). Handles symlink loops, hidden files, massive directory trees. `filter_entry` for efficient early-exit. [VERIFIED: crates.io `walkdir` 2.5.0] |
| **serde** / **serde_json** | 1.0+ (workspace) | Serialization for Manifest, API responses, SQLite JSON blobs | Already in workspace. Required for `#[derive(Deserialize)]` on SkillManifest. [VERIFIED: workspace Cargo.toml] |
| **chrono** | 0.4+ (workspace) | Timestamps for skill load/unload events | Already in workspace. Used for `loaded_at`, `updated_at` in SkillIndex. [VERIFIED: workspace Cargo.toml] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **dashmap** | 6 (workspace) | Concurrent read-heavy state for SkillRegistry | Same pattern as InstancePool and ToolRegistry. Arc<DashMap<TenantId, SkillState>> for loaded skill state. [VERIFIED: workspace Cargo.toml] |
| **sqlx** | 0.9 (workspace) | SQLite skill_index table for cached metadata | list_skills endpoint queries index instead of scanning filesystem. Migration for `skill_index` table follows SessionRepository pattern. [VERIFIED: workspace Cargo.toml] |
| **uuid** | 1.0 (workspace) | SkillId type (UUID v7) | Mirrors SessionId/TenantId/ToolId pattern. Time-ordered for DB index friendliness. [VERIFIED: workspace Cargo.toml] |
| **axum** | 0.8 (workspace) | REST endpoints for skill load/unload/list/inspect | Server crate mounts skill routes. Json extractor for request bodies. [VERIFIED: workspace Cargo.toml] |
| **tracing** | 0.1 (workspace) | Instrumentation for skill load/unload events | Spans for skill operations. Phase 9 OBS-01 requires these events. [VERIFIED: workspace Cargo.toml] |
| **thiserror** | 2 (via gray_matter) | SkillValidationError derive | gray_matter already depends on thiserror. Use for structured validation errors. [VERIFIED: gray_matter Cargo.toml dependency tree] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| **gray_matter + yaml-rust2** | serde_yaml 0.9 (deprecated) | serde_yaml is officially unmaintained since March 2024. Would work for MVP but creates tech debt. gray_matter handles the `---` delimiter extraction that would otherwise be hand-rolled. |
| **gray_matter** | Hand-rolled `---` split + serde for YAML | 30+ lines of string splitting logic that gray_matter already tests. gray_matter is 0.3.2 (stable, not bleeding edge). The hand-roll risk is edge cases (leading whitespace before `---`, CRLF line endings, etc.). |
| **walkdir 2.5** | std::fs::read_dir recursive | std's read_dir does not handle symlink loops, has no `filter_entry` for efficient early-exit, and produces slower iterations for deep directory trees. walkdir is a proven, well-maintained crate. |
| **yaml-rust2 (via gray_matter)** | serde_yml 0.0.13 | serde_yml is also DEPRECATED — thin shim forwarding to libyml which is also deprecated. Not a viable alternative. |

**Installation:**
```bash
# Add to workspace Cargo.toml [workspace.dependencies]:
gray_matter = { version = "0.3", default-features = true }  # yaml feature on by default
walkdir = "2.5"

# In crates/jadepaw-skill/Cargo.toml [dependencies]:
gray_matter = { workspace = true }
walkdir = { workspace = true }
```

**Version verification:**
```bash
# Already confirmed on crates.io:
# gray_matter: 0.3.2 (depends on yaml-rust2 0.10, thiserror 2, serde 1)
# walkdir: 2.5.0 (BurntSushi, Unlicense/MIT)
# yaml-rust2: 0.11.0 available but gray_matter pins 0.10
```

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | slopcheck | Disposition |
|---------|----------|-----|-----------|-------------|-----------|-------------|
| gray_matter | crates.io | 3+ yrs (v0.1.0) | Established | github.com/the-alchemists-of-arland/gray-matter-rs | OK | Approved |
| walkdir | crates.io | 8+ yrs | 100M+ total | github.com/BurntSushi/walkdir | OK | Approved |
| yaml-rust2 | crates.io | 3+ yrs | Established | github.com/Ethiraric/yaml-rust2 | OK | Approved (transitive via gray_matter) |

**Packages removed due to slopcheck [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
                    SKILL.md files (.jadepaw/skills/)
                           │
                           ▼
┌──────────────────────────────────────────────────────────────┐
│  Startup: walkdir scan                                        │
│  ┌─────────────┐    ┌─────────────┐    ┌──────────────────┐  │
│  │ walkdir      │───▶│ gray_matter │───▶│ SkillIndex::sync │  │
│  │ (file find)  │    │ (parse YAML)│    │ (SQLite upsert)  │  │
│  └─────────────┘    └─────────────┘    └──────────────────┘  │
└──────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────┐
│  REST API (axum routes in jadepaw-server)                     │
│                                                               │
│  POST /skills/load       ──▶ SkillManager::load()             │
│  POST /skills/unload     ──▶ SkillManager::unload()           │
│  GET  /skills/list       ──▶ SkillIndex::query()  (SQLite)    │
│  GET  /skills/inspect/{n}──▶ SkillManager::inspect()          │
└──────────────────────────────────────────────────────────────┘
                           │
                           ▼
┌──────────────────────────────────────────────────────────────┐
│  SkillManager (in-memory, Arc<DashMap>)                       │
│                                                               │
│  ┌────────────────────┐   ┌──────────────────────────┐       │
│  │ SkillRegistry      │   │ ActiveSkillState          │       │
│  │ (DashMap: skill_id │   │ per tenant:               │       │
│  │  -> Arc<Skill>)    │   │   loaded_skills: Vec<...> │       │
│  │                    │   │   pending_swap: Option<..>│       │
│  │ merge_active()     │   │                           │       │
│  │  -> (system_prompt │   │ check_pending_swap()      │       │
│  │      block,        │   │  -> Option<SkillSwap>     │       │
│  │      tool_list)    │   │                           │       │
│  └────────────────────┘   └──────────────────────────┘       │
└──────────────────────────────────────────────────────────────┘
                           │
                           ▼  (skill context injected)
┌──────────────────────────────────────────────────────────────┐
│  react_loop (jadepaw-agent/src/loop.rs)                       │
│                                                               │
│  for each turn:                                               │
│    1. check_pending_swap() -> if Some, rebuild messages[0]    │
│    2. SkillManager::merge_active() -> augmented system prompt │
│    3. build_system_prompt_with_tools(augmented, tools)        │
│    4. stream_llm_response()                                   │
│    5. parse_next_action() -> Finish / Act / ContinueThinking  │
│                                                               │
│  compress_context() preserves messages[0] (system prompt)     │
└──────────────────────────────────────────────────────────────┘
```

### Recommended Project Structure

```
crates/jadepaw-skill/
├── src/
│   ├── lib.rs              # crate docs, re-exports
│   ├── manifest.rs         # SkillManifest struct, serde Deserialize
│   ├── parser.rs           # gray_matter wrapper, YAML -> Manifest + body
│   ├── validation.rs       # SkillValidationError, field validation rules
│   ├── manager.rs          # SkillManager: load/unload/list/merge_active
│   ├── registry.rs         # SkillRegistry: DashMap<TenantId, ActiveSkillState>
│   ├── loader.rs           # SkillLoader: file I/O, walkdir scanning
│   ├── index.rs            # SkillIndex: SQLite cache sync (bulk upsert)
│   └── injector.rs         # System prompt builder: XML block formatting
│
crates/jadepaw-core/src/
│   ├── skill_types.rs      # SkillId (new), SkillManifest, SkillValidationError
│   └── types.rs            # + SkillId type definition
│
crates/jadepaw-agent/src/
│   ├── lib.rs              # + SkillManager injection into run_agent()
│   ├── llm.rs              # + build_skill_augmented_prompt()
│   └── loop.rs             # + pending_skill_change check at turn top
│
crates/jadepaw-db/src/
│   ├── skill_repository.rs # SkillRepository trait (dual-key pattern)
│   ├── skill_models.rs     # SkillIndexRecord, SkillIndexSummary
│   ├── sqlite_skill_repo.rs# SqliteSkillRepo impl
│   └── migrations/         # + skill_index table migration
│
crates/jadepaw-server/src/
│   └── routes/skills.rs    # axum Router for skill API endpoints
```

### Pattern 1: YAML Frontmatter Parsing with gray_matter

**What:** Extract YAML frontmatter (between `---` delimiters) and Markdown body from a SKILL.md file string, deserializing into a strongly-typed struct with custom validation.

**When to use:** Every SKILL.md file read (startup walkdir scan, API load request, API inspect).

**Example:**
```rust
// Source: gray_matter 0.3.2 docs (docs.rs/gray_matter/0.3.2)
use gray_matter::engine::YAML;
use gray_matter::Matter;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SkillManifest {
    name: String,
    description: String,
    #[serde(default)]
    tools: Vec<String>,
    #[serde(default)]
    constraints: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Map<String, serde_json::Value>>,
}

fn parse_skill_md(content: &str) -> Result<(SkillManifest, String), SkillValidationError> {
    let matter = Matter::<YAML>::new();
    let result = matter.parse(content);
    // result.data: Option<Pod> or deserialize directly
    // result.content: String (body after frontmatter)
    
    let manifest: SkillManifest = matter
        .parse_with_struct::<SkillManifest>(content)
        .map_err(|e| parse_error_to_validation(e))?
        .data;
    
    let body = matter.parse(content).content;
    Ok((manifest, body))
}
```

**Critical note:** gray_matter's `parse_with_struct` requires the custom struct be `Deserialize`. But for jadepaw's custom validation (name regex, description length, tool references), parse into `serde_json::Value` first (via `Pod`), run validation logic against raw values, then convert to `SkillManifest`. This gives us the validation granularity D-01 requires.

### Pattern 2: Late-Binding System Prompt Rebuild at Turn Boundaries

**What:** Rebuild `messages[0]` (system prompt) at the top of each ReAct turn by calling `SkillManager::merge_active()`, which produces the augmented system prompt block with all active skills. On pending swap, atomically replace before the LLM call.

**When to use:** Top of react_loop() for loop, before every LLM call.

**Example:**
```rust
// Integration into react_loop (jadepaw-agent/src/loop.rs)
// At the top of the for-turn loop, before should_compress/stream_llm_response:

// Check for pending skill swap (D-07)
if let Some(skill_swap) = skill_manager.check_pending_swap(tenant_id) {
    let augmented = skill_swap.new_system_prompt;
    messages[0] = ChatCompletionRequestSystemMessage::from(augmented).into();
    
    // Rebuild tool list from merged skill declarations
    let merged_tools = skill_swap.merged_tool_list;
    // Update ToolRegistry or rebuild the augmented prompt
}
```

### Pattern 3: SkillRegistry with DashMap (Concurrent State)

**What:** Thread-safe registry of loaded skills per tenant, using `Arc<DashMap<TenantId, ActiveSkillState>>` -- identical to the InstancePool / ToolRegistry concurrency pattern.

**When to use:** SkillManager's internal state holding. Read-heavy (every turn reads active skills), write-rare (only on API load/unload).

**Example:**
```rust
use dashmap::DashMap;
use jadepaw_core::{SkillId, TenantId};

struct ActiveSkillState {
    loaded_skills: Vec<LoadedSkill>,
    pending_swap: Option<SkillSwap>,
}

struct LoadedSkill {
    skill_id: SkillId,
    manifest: SkillManifest,
    body: String,           // Markdown instructions
    priority: u8,
    loaded_at: chrono::DateTime<chrono::Utc>,
}

struct SkillSwap {
    new_system_prompt: String,
    merged_tool_list: Vec<ToolDefinition>,
}

impl SkillRegistry {
    fn new() -> Self {
        Self {
            states: DashMap::new(),
        }
    }
}
```

### Pattern 4: ToolRegistry register() - Panic to Result Migration

**What:** Change `ToolRegistry::register()` signature from `-> ToolId` (panics on duplicate) to `-> Result<ToolId, ToolConflictError>` (returns structured error on conflict). This is required by D-04 for priority-based union merging.

**When to use:** When skills declare tools that may conflict with existing registrations.

**Example:**
```rust
// Current (Phase 4) — panics on duplicate:
pub fn register(&self, tool: Arc<dyn Tool>) -> ToolId {
    if self.name_index.contains_key(&name) {
        panic!("ToolRegistry: duplicate tool name '{}'", name);
    }
    // ...
}

// New (Phase 6) — returns Result:
#[derive(Debug, thiserror::Error)]
pub enum ToolConflictError {
    #[error("duplicate tool name '{name}': existing priority {existing}, new priority {new}")]
    DuplicateTool { name: String, existing: u8, new: u8 },
    #[error("tool '{name}' already registered with incompatible schema")]
    SchemaMismatch { name: String },
}

pub fn register(&self, tool: Arc<dyn Tool>, priority: u8) -> Result<ToolId, ToolConflictError> {
    if let Some(existing) = self.name_index.get(&name) {
        // D-04: same name, same schema → higher priority wins
        // D-04: same name, different schema → warn + skip
        return Err(ToolConflictError::DuplicateTool { ... });
    }
    // ...
}
```

### Anti-Patterns to Avoid
- **Blocking I/O in async context:** walkdir scanning at startup runs on a dedicated thread via `tokio::task::spawn_blocking` since it performs filesystem operations. Never call `walkdir` iterators directly in an async function.
- **Mixing gray_matter's Pod and parse_with_struct incorrectly:** gray_matter's default YAML engine uses yaml-rust2's `Yaml` type under the hood. The `Pod` type provides `as_string()`, `as_hash()`, etc. for validation before struct deserialization. Parse into Pod first for validation, then convert.
- **Forgetting messages[0] preservation in compress_context:** Phase 5's `compress_context` already preserves `messages[0]` (the system prompt). Skill injection adds content to messages[0] -- verify that after compression, the skill instructions are still present.
- **Parsing YAML without proper error location reporting:** yaml-rust2 errors carry line/column information through the `ScanError` type. Always extract and surface these in `SkillValidationError` so users can locate issues in their SKILL.md files (success criterion 4).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| `---` delimiter splitting in Markdown | String::split("---") + edge case handling | gray_matter::Matter::<YAML>::new().parse() | gray_matter handles leading whitespace, CRLF, multiple `---` blocks, opening-fence detection. Hand-rolling this is 30+ lines of fragile string parsing. |
| YAML parsing from frontmatter | serde_yaml (deprecated) or manual regex | yaml-rust2 (via gray_matter) | serde_yaml was deprecated in March 2024. gray_matter uses yaml-rust2 which is actively maintained, YAML 1.2 compliant, and handles the frontmatter extraction in one call. |
| Recursive directory walking | std::fs::read_dir with manual recursion | walkdir 2.5 | walkdir handles symlink loops, file descriptor limits, hidden files, `filter_entry` for early exit. Hand-rolling misses these edge cases. |
| Frontmatter field validation regex | `if !s.chars().all(...)` | Dedicated validation function with clear error variants | The name field validation rules (no uppercase, no leading/trailing/consecutive hyphens) are nuanced enough to warrant explicit tests. A single boolean check hides which rule was violated. |
| System prompt string concatenation | format!("{}\n{}", base, skill_text) | Structured XML block builder (`SkillInjector`) | D-02 requires `<skill_instructions>` XML blocks with name/version/priority attributes. A dedicated builder ensures consistent formatting and renders cleanly for auditing. |
| Multi-skill context merging | Vec::extend or manual merge loop | SkillManager::merge_active() with sorted+dedup logic | Priority sorting, conflict resolution, and deduplication need a central coordinator. Spreading this across callers creates inconsistency. |
| Tool validation at load time | Option<String> error messages | SkillValidationError enum with field-level granularity | D-05 requires structured errors (`{ field, reason }`) so API consumers can display user-friendly messages. String errors are not machine-parseable. |

**Key insight:** YAML frontmatter parsing is deceptive. It looks like "split on `---` and parse YAML" -- but the real complexity is in edge cases (leading whitespace, CRLF line endings, empty frontmatter blocks, multiple `---` sequences in body text). gray_matter handles all of these.

## Common Pitfalls

### Pitfall 1: serde_yaml Deprecation Blindness

**What goes wrong:** Installing `serde_yaml = "0.9"` because it's mentioned in CLAUDE.md and CONTEXT.md, only to discover it's deprecated and unmaintained.
**Why it happens:** CLAUDE.md and CONTEXT.md were written before the deprecation was widely known. The crate still exists on crates.io and `cargo add` succeeds.
**How to avoid:** Use `gray_matter = "0.3"` which internally uses `yaml-rust2 0.10` for YAML parsing. The workspace Cargo.toml already notes "YAML support deferred to Phase 6 (serde_yaml is deprecated since March 2024)."
**Warning signs:** `cargo add serde_yaml` succeeds but produces a warning about deprecation. The crate docs at docs.rs/serde_yaml show "This project is no longer maintained."

### Pitfall 2: gray_matter Deserialization vs Validation Timing

**What goes wrong:** Calling `matter.parse_with_struct::<SkillManifest>()` directly, which fails with a serde deserialization error that doesn't tell the user WHICH custom validation rule was violated (only that a field was missing or the wrong type).
**Why it happens:** gray_matter's `parse_with_struct` uses serde's generic deserialization, which produces error messages like "missing field `name`" but cannot express jadepaw-specific rules like "name must be kebab-case."
**How to avoid:** Parse into gray_matter's `Pod` first (key-value access), run jadepaw validation logic field-by-field, produce `SkillValidationError` with specific field/reason pairs, THEN construct `SkillManifest`. See Parser Pattern in Code Examples.
**Warning signs:** Test failure "expected SkillValidationError::InvalidName, got serde::de::Error."

### Pitfall 3: Mid-Session Swap During In-Flight LLM Call

**What goes wrong:** Skill swap changes `messages[0]` while `stream_llm_response()` is still awaiting tokens, causing the next assistant message to be based on the old system prompt but the observation message to be appended under the new system prompt.
**Why it happens:** The LLM call is async and takes several seconds. If swap happens concurrently with an in-flight call, the message sequence becomes inconsistent.
**How to avoid:** D-07 specifically mandates checking `pending_skill_change` at turn boundaries, NOT mid-stream. The `react_loop` calls `check_pending_swap()` before `stream_llm_response()`, never during. The lock is held only for the swap check, not for the entire LLM call.
**Warning signs:** Skill swap API called during active streaming, resulting in garbled agent responses.

### Pitfall 4: walkdir Blocking in Async Context

**What goes wrong:** Calling `WalkDir::new(path).into_iter()` directly in an async function blocks the tokio runtime thread, starving other tasks.
**Why it happens:** walkdir is synchronous. Its iterator performs filesystem syscalls that block the thread.
**How to avoid:** Wrap startup scanning in `tokio::task::spawn_blocking()`. Per D-06, only API-driven reloads happen at runtime (no filesystem watcher), so the blocking scan only occurs at startup.
**Warning signs:** tokio console showing a worker thread blocked for >100ms during startup. Other requests timing out during skill scan.

### Pitfall 5: Skill Name vs Directory Name Mismatch

**What goes wrong:** A SKILL.md file at `~/.jadepaw/skills/tenant-123/my-skill/SKILL.md` has `name: other-name` in its YAML frontmatter.
**Why it happens:** The Agent Skills spec says `name` must match the parent directory name, but users can create files manually and get this wrong.
**How to avoid:** Explicitly validate at parse time: `if manifest.name != dir_name { return Err(SkillValidationError::NameDirectoryMismatch { ... }) }`. Reject with clear error.
**Warning signs:** Skill loads successfully but API inspect returns a different name than the directory. Confusion in list_skills output.

## Code Examples

Verified patterns from official sources:

### Parsing SKILL.md with Validation

```rust
// Source: gray_matter 0.3.2 docs (docs.rs/gray_matter/0.3.2) + Agent Skills spec (agentskills.io/specification)
use gray_matter::{Matter, engine::YAML, Pod};
use std::path::Path;

/// Parse a SKILL.md file content into a validated SkillManifest and body.
fn parse_skill_file(
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
    
    // Step 3: Validate required fields exist
    let name = data["name"].as_string()
        .ok_or(SkillValidationError::MissingField { field: "name".into() })?;
    let description = data["description"].as_string()
        .ok_or(SkillValidationError::MissingField { field: "description".into() })?;
    
    // Step 4: Agent Skills spec name validation rules
    validate_skill_name(name)?;
    
    // Step 5: Name must match directory (spec requirement)
    if name != dir_name {
        return Err(SkillValidationError::NameDirectoryMismatch {
            expected_name: dir_name.to_string(),
            actual_name: name.to_string(),
        });
    }
    
    // Step 6: Description length check (spec: max 1024)
    if description.len() > 1024 {
        return Err(SkillValidationError::FieldTooLong {
            field: "description".into(),
            max: 1024,
            actual: description.len(),
        });
    }
    
    // Step 7: Collect jadepaw extension fields
    let tools = extract_tools_array(&data);
    let constraints = data["constraints"].as_string();
    let version = data["version"].as_string();
    let author = data["author"].as_string();
    
    Ok((SkillManifest {
        name: name.to_string(),
        description: description.to_string(),
        tools,
        constraints,
        version,
        author,
        metadata: None,
        source_path: file_path.to_path_buf(),
    }, body))
}

/// Agent Skills spec name validation:
/// - 1-64 characters
/// - lowercase letters, numbers, hyphens only
/// - must not start or end with hyphen
/// - must not contain consecutive hyphens
fn validate_skill_name(name: &str) -> Result<(), SkillValidationError> {
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

### Walkdir Startup Scanning with SQLite Index

```rust
// Source: walkdir 2.5 docs (docs.rs/walkdir/2.5.0)
use walkdir::WalkDir;
use std::path::Path;

/// Scan all SKILL.md files under the skills root directory.
/// Runs at server startup via spawn_blocking.
fn scan_skills_root(skills_root: &Path) -> Vec<SkillFileEntry> {
    let mut entries = Vec::new();
    
    for entry in WalkDir::new(skills_root)
        .follow_links(false)  // no symlink following for security
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() == "SKILL.md")
    {
        let path = entry.path().to_path_buf();
        // Extract tenant_id and skill_name from path:
        // skills/<tenant_id>/<skill_name>/SKILL.md
        let parent_dir = entry.path().parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string());
        
        if let Some(skill_name) = parent_dir {
            entries.push(SkillFileEntry {
                path,
                skill_name,
                // tenant_id extracted from grandparent dir
            });
        }
    }
    
    entries
}
```

### XML Injection Format for System Prompt

```rust
// Source: D-02 specification from CONTEXT.md
/// Build the merged system prompt block with all active skills.
/// Uses structured XML tags for auditability.
fn build_skill_context_block(
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

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| serde_yaml 0.9 (deprecated) | yaml-rust2 0.10 (via gray_matter 0.3) | March 2024 | serde_yaml is unmaintained. gray_matter wraps YAML parsing + frontmatter extraction in one API call |
| serde_yml 0.0.x | yaml-rust2 | 2025-2026 | serde_yml was a compatibility shim that is also now deprecated |
| `AgentRequest.context: Option<String>` (free-form string) | `AgentRequest.skills: Option<Vec<SkillId>>` + structured skill context injection | Phase 6 | Structured multi-skill injection replaces ad-hoc context strings. The `context` field remains for backward compat but skill injection is the primary mechanism |
| ToolRegistry::register() panics on duplicate | `register() -> Result<ToolId, ToolConflictError>` with priority | Phase 6 | Skills can declare overlapping tool sets; union merge replaces panic-on-duplicate with priority-based resolution |

**Deprecated/outdated:**
- **serde_yaml:** Entirely deprecated since March 2024. Do not use.
- **serde_yml:** Also deprecated. Was a thin shim, now abandoned.
- **frontmatter crate (0.4.0):** Uses yaml-rust (original, not yaml-rust2) which is less actively maintained. gray_matter is a more complete solution with Pod access for validation.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | gray_matter 0.3.2's `Matter::<YAML>::parse()` correctly handles SKILL.md files with `---` frontmatter delimiters | Standard Stack | Low -- gray_matter is designed for exactly this purpose (Jekyll/Hugo frontmatter) |
| A2 | yaml-rust2 (via gray_matter) provides adequate parse error location information (line/column) for user-facing error messages | Standard Stack | Medium -- if yaml-rust2 errors are opaque, we may need to wrap with serde_path_to_error or add line-column scanning |
| A3 | The Agent Skills spec will not change in ways that break jadepaw's extension fields (`tools`, `constraints`, `version`, `author`) | Standard Stack | Low -- the spec explicitly allows `metadata` for arbitrary extensions and says "Clients can use this to store additional properties" |
| A4 | SkillManager integration into react_loop requires threading `Arc<SkillManager>` through run_agent() parameters | Architecture Patterns | Low -- run_agent() already accepts tool_registry and other Arcs; adding one more parameter follows the established pattern |
| A5 | ToolRegistry's DashMap mutation from `register()` returning `Result` instead of panicking does not affect existing callers | Architecture Patterns | Medium -- existing callers in Phase 3/4 tests may need updating if they rely on `register()` returning `ToolId` directly |
| A6 | SQLite `skill_index` table can reuse the same connection pool pattern as `SessionRepository` | Architecture Patterns | Low -- both tables are in the same SQLite database; same pool, same WAL mode |

## Open Questions

1. **gray_matter YAML error granularity for user-facing validation**
   - What we know: gray_matter uses yaml-rust2 under the hood. yaml-rust2's `ScanError` carries `Marker` with line/column info. gray_matter wraps this.
   - What's unclear: Whether gray_matter surfaces the underlying `Marker` through its error types, or if we need to go around gray_matter for parse error location reporting.
   - Recommendation: Test with an invalid YAML SKILL.md early in Wave 0. If gray_matter's errors are opaque, add a pre-parse step that extracts the frontmatter block manually, feeds it to yaml-rust2 directly for validation, and captures the `ScanError` with line numbers.

2. **Skill extension fields: `metadata` map vs top-level fields**
   - What we know: D-01 specifies `tools`, `constraints`, `version`, `author` as top-level jadepaw fields. The Agent Skills spec says unknown fields should be "silently ignored" by standard validators, but jadepaw parsers should enforce them.
   - What's unclear: Should jadepaw also support arbitrary metadata injection through the standard `metadata:` map, or only through the curated top-level fields?
   - Recommendation: Support both. The `metadata:` map passes through unvalidated (standard field). Top-level jadepaw fields get structured validation. This is already implied by D-01 ("扩展字段在标准 validator 中被静默忽略，在 jadepaw parser 中做结构化校验").

3. **Skill load during active session: blocking vs non-blocking**
   - What we know: D-07 says mid-session swaps happen at turn boundaries (no in-flight LLM call interruption). D-08 says new skills are parsed/validated in a staging area.
   - What's unclear: Can a skill load be requested while a session is PAUSED (stored in DB, no active loop)? The API should probably support this -- load the skill, the session picks it up on resume.
   - Recommendation: Skill load/unload is independent of session state. The API accepts `tenant_id + skill_name`, not `session_id`. The session picks up current active skills when it starts or resumes a new turn.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust | All crates | ✓ | 1.85+ (workspace MSRV) | — |
| cargo (with crates.io access) | gray_matter, walkdir dependencies | ✓ | via rustup | — |
| ~/.jadepaw/ directory | Skill file storage (D-09) | ✗ | — | Auto-create at server startup with `std::fs::create_dir_all` |
| SQLite | SkillIndex cache (D-09) | ✓ | via sqlx 0.9 (workspace) | — |

**Missing dependencies with no fallback:** none (all tooling is within the Rust/Cargo ecosystem)
**Missing dependencies with fallback:** `~/.jadepaw/` directory — auto-created at startup

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust's built-in test framework) + rstest (workspace) |
| Config file | Per-crate Cargo.toml `[dev-dependencies]` |
| Quick run command | `cargo test -p jadepaw-skill` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SKILL-01 | Parse valid SKILL.md with all jadepaw extension fields | unit | `cargo test -p jadepaw-skill -- test_parse_valid_skill` | No (Wave 0) |
| SKILL-01 | Reject SKILL.md with missing `name` field | unit | `cargo test -p jadepaw-skill -- test_reject_missing_name` | No (Wave 0) |
| SKILL-01 | Reject SKILL.md with invalid YAML, return parse location | unit | `cargo test -p jadepaw-skill -- test_reject_invalid_yaml` | No (Wave 0) |
| SKILL-01 | Reject SKILL.md with name not matching directory | unit | `cargo test -p jadepaw-skill -- test_name_directory_mismatch` | No (Wave 0) |
| SKILL-01 | Accept SKILL.md with only name+description (minimal) | unit | `cargo test -p jadepaw-skill -- test_parse_minimal_skill` | No (Wave 0) |
| SKILL-02 | Load skill via API, verify system prompt contains skill instructions | integration | `cargo test -p jadepaw-skill -- test_load_skill_injects_prompt` | No (Wave 0) |
| SKILL-02 | Swap skill mid-session, verify behavior changes next turn | integration | `cargo test -p jadepaw-agent -- test_mid_session_skill_swap` | No (Wave 0) |
| SKILL-02 | Unload skill, verify agent reverts to default behavior | integration | `cargo test -p jadepaw-agent -- test_unload_skill_reverts` | No (Wave 0) |
| SKILL-02 | Multiple skills loaded, verify union tool merge | unit | `cargo test -p jadepaw-skill -- test_multi_skill_tool_union` | No (Wave 0) |
| SKILL-02 | Tool conflict with same schema, higher priority wins | unit | `cargo test -p jadepaw-agent -- test_tool_conflict_priority` | No (Wave 0) |
| SKILL-02 | Skill declares non-existent ToolId, load rejected | unit | `cargo test -p jadepaw-skill -- test_reject_unknown_tool` | No (Wave 0) |

### Sampling Rate
- **Per task commit:** `cargo test -p jadepaw-skill`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/jadepaw-skill/tests/` — test directory does not exist yet
- [ ] `crates/jadepaw-skill/tests/fixtures/` — SKILL.md test fixtures (valid/invalid/minimal)
- [ ] `crates/jadepaw-skill/Cargo.toml` — dev-dependencies for gray_matter test fixtures, tempfile
- [ ] `crates/jadepaw-agent/tests/skill_integration.rs` — integration tests for skill injection into react_loop
- [ ] Framework install: `cargo test` already works at workspace level — no new test framework needed

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | Phase 6 has no auth endpoints; auth is deferred to Phase 9 gateway middleware |
| V3 Session Management | No | Skill loading is stateless per-request; no session token is created for skill operations |
| V4 Access Control | Yes | Tenant isolation via tenant_id on every skill operation (D-10, D-11). SkillManager gates all operations on tenant_id. Path validation (Phase 2) prevents directory traversal in skill file paths |
| V5 Input Validation | Yes | YAML frontmatter is parsed and validated against the Agent Skills spec schema. Name field constrained to `[a-z0-9-]+` regex. Description length capped at 1024 chars. Invalid input rejected before any state mutation (D-05, D-08) |
| V6 Cryptography | No | No cryptographic operations in the skill system |

### Known Threat Patterns for Skill System

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Path traversal in skill file loading (`../../../etc/shadow` in skill_name) | Tampering | Canonicalize + prefix check (Phase 2 infrastructure). Skill file paths are constructed from validated tenant_id + sanitized skill_name, never from user-provided raw paths |
| YAML bomb (deeply nested YAML causing OOM during parse) | Denial of Service | yaml-rust2 has configurable recursion limit. Set a maximum YAML document size (e.g., 1MB) before parsing |
| Skill with malicious Markdown body executing code in LLM prompt injection | Information Disclosure | This is an LLM-level concern. Skills inject natural language instructions, not executable code. The LLM follows skill instructions within its context window. This is an accepted risk of the "natural language programming" model |
| Cross-tenant skill access (tenant A loading tenant B's skills) | Information Disclosure | All SkillManager methods require tenant_id parameter. Path construction uses `~/.jadepaw/skills/<tenant_id>/`. Path validation (Phase 2) enforces the tenant prefix boundary |
| Race condition in mid-session skill swap | Tampering | D-07 specifies swap at turn boundary only. `pending_skill_change` flag is set atomically and checked once per turn. In-flight LLM calls are never interrupted. Staging validation (D-08) ensures the new skill is fully valid before swap |
| Skill file modified externally during active session | Tampering | D-06 specifies pure API-driven loading. Filesystem is only for persistent storage. Once a skill is loaded into memory (`Arc<DashMap>`), the in-memory copy is the active version. To pick up filesystem changes, user must explicitly call reload |

## Sources

### Primary (HIGH confidence)
- [Agent Skills Specification](https://agentskills.io/specification) — Complete SKILL.md format: required fields (name, description), optional fields (license, compatibility, metadata, allowed-tools), naming constraints, validation rules, progressive disclosure model
- [gray_matter 0.3.2 docs](https://docs.rs/gray_matter/0.3.2) — YAML frontmatter extraction API: Matter<YAML>::new(), parse(), parse_with_struct(), Pod access for validation-before-deserialization
- [walkdir 2.5.0 docs](https://docs.rs/walkdir/2.5.0) — Recursive directory iteration: WalkDir::new(), filter_entry(), symlink handling, IntoIter API
- [yaml-rust2 0.11.0](https://docs.rs/yaml-rust2) — YAML 1.2 parsing engine used by gray_matter: YamlLoader::load_from_str(), ScanError with Marker location
- [Workspace Cargo.toml](/Users/yue.weny/finalanswer/jadepaw/jadepaw/Cargo.toml) — workspace dependencies: serde 1.0, serde_json 1.0, dashmap 6, uuid 1.0, chrono 0.4, sqlx 0.9, axum 0.8

### Secondary (MEDIUM confidence)
- [jadepaw-agent/src/lib.rs](crates/jadepaw-agent/src/lib.rs:66-171) — run_agent() entry point: system prompt at lines 99-108, ToolRegistry integration at line 99, AgentRequest.context at line 77. Primary injection surface
- [jadepaw-agent/src/llm.rs](crates/jadepaw-agent/src/llm.rs:86-137) — build_initial_messages(), build_system_prompt_with_tools(). Skill context replaces the current context:Option<String> approach
- [jadepaw-agent/src/loop.rs](crates/jadepaw-agent/src/loop.rs:129-372) — react_loop(): per-turn boundaries, messages[0] is mutable, context compression preserves messages[0]
- [jadepaw-agent/src/tool_registry.rs](crates/jadepaw-agent/src/tool_registry.rs:33-65) — ToolRegistry with DashMap, register() panics on duplicate (needs Result migration per D-04)
- [jadepaw-agent/src/window.rs](crates/jadepaw-agent/src/window.rs:100-207) — compress_context() preserves messages[0] (system prompt), ensures skill instructions survive compression
- [jadepaw-db/src/repository.rs](crates/jadepaw-db/src/repository.rs:1-95) — SessionRepository trait pattern: dual-key isolation, async_trait, save/load/list/delete/update_status
- [jadepaw-core/src/types.rs](crates/jadepaw-core/src/types.rs:1-119) — Type pattern: UUID v7 newtypes with Deref, Display, Default. SkillId should follow same pattern
- [jadepaw-core/src/agent_types.rs](crates/jadepaw-core/src/agent_types.rs:1-183) — AgentRequest.context field (line 31), AgentResponse, ReActStep enum. context field extension point for skill references
- [gray_matter Cargo.toml](https://raw.githubusercontent.com/the-alchemists-of-arland/gray-matter-rs/main/Cargo.toml) — Dependency tree: yaml-rust2 0.10 (optional, default on), serde 1, thiserror 2

### Tertiary (LOW confidence)
- gray_matter parse error location reporting — web docs show Pod-based access patterns but do not explicitly document line/column error extraction. Flagged for Wave 0 testing (Open Question 1)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — All libraries verified on crates.io with version numbers. gray_matter 0.3.2, walkdir 2.5.0 both pass slopcheck. yaml-rust2 is the active YAML parser (serde_yaml deprecated).
- Architecture: HIGH — Integration surface fully mapped through reading actual source code (run_agent, react_loop, llm, tool_registry, window). Concurrency patterns documented (Arc<DashMap>, turn-boundary swap). Context compression verified to preserve messages[0].
- Pitfalls: MEDIUM — Five major pitfalls identified from codebase analysis + ecosystem knowledge. The gray_matter error granularity question (Open Question 1) needs Wave 0 validation. The ToolRegistry Result migration (Pitfall: existing callers may break) needs explicit test verification.

**Research date:** 2026-06-05
**Valid until:** 2026-07-05 (30 days; stable crate versions, well-established spec)