# Phase 6: Skill System - Context

**Gathered:** 2026-06-05
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase adds a declarative skill system: users define agent behaviors through SKILL.md files (YAML frontmatter + Markdown natural language instructions), load them at runtime via API, and swap between skills mid-session without restarting. Skills are data-driven configuration — not compiled Wasm in v1 — injected as structured context into the agent's system prompt. Multiple skills can be active simultaneously with merged tool declarations and instruction contexts.

**Success Criteria (from ROADMAP.md):**
1. User creates SKILL.md (YAML frontmatter: name, description, tools, constraints + Markdown body: natural language instructions), places it in the skills directory, and the agent immediately adopts the described behavior on the next invocation
2. User loads a "code reviewer" skill, has a conversation, then swaps to a "data analyst" skill mid-session — agent behavior changes without restart
3. User unloads a skill, and the agent reverts to default behavior on the next invocation
4. Invalid YAML frontmatter is rejected at load time with clear error message indicating parse failure location
5. Multiple skills loaded simultaneously — agent correctly merges tool declarations and instruction contexts

**Requirements covered:** SKILL-01, SKILL-02

</domain>

<decisions>
## Implementation Decisions

### SKILL.md Format Design
- **D-01:** Agent Skills 标准 + 精选扩展字段。遵循 agentskills.io 开放标准（`name` + `description` 必填），顶层增加 jadepaw 特有字段：`tools`（结构化工具声明，映射到 ToolRegistry）、`constraints`（自然语言约束，注入 system prompt）、`version`（语义化版本，为 v2 版本管理预留）、`author`（归属）。扩展字段在标准 validator 中被静默忽略，在 jadepaw parser 中做结构化校验。YAML frontmatter 缺失必填字段在解析阶段即被拒绝（符合 success criterion 4）。

### Skill Context Injection
- **D-02:** Hybrid Layered Injection（XML tag 结构化包裹）。所有 active skill 的指令注入到 system prompt 中，使用 `<skill_instructions>` XML block 包裹每条 skill（含 name、version、priority 属性）。多 skill 按 priority 排序（默认 0，高优先级覆盖低冲突声明）。提供结构化分隔和可审计性。
- **D-03:** Late-Binding 动态重建。每轮 ReAct turn 开始前从 `SkillManager::merge_active()` 重建 system prompt，替换 `messages[0]`。支持 mid-session skill swap —— 当前 turn 完成后，下一 turn 前原子替换。`compress_context`（Phase 5）保留 `messages[0]` 的逻辑确保压缩后 skill 指令不丢失。

### Tool Declaration Merging
- **D-04:** Union 合并策略。多个 skill 的工具声明取并集，通过 `ToolRegistry`（已有 DashMap 并发安全架构）注册。同一 tool 重名时：priority 高的 skill 版本生效，schema 不同时 warn + skip。`register()` panic-on-duplicate 改为 `Result`。
- **D-05:** Skill 加载时验证工具可用性。声明引用的 `ToolId` 必须在 `ToolRegistry` 中存在 —— 缺失则拒绝加载，返回结构化 `SkillValidationError { field, reason }`。不静默降级。

### Hot-Loading & Mid-Session Swap
- **D-06:** 纯 API 驱动。load/unload/reload/list 通过 REST API 操作（aligns with jadepaw Web-first architecture）。无文件监听（notify）或轮询 —— 文件系统仅作为持久化存储层。Phase 7+ 可扩展 Hybrid 模式（API + file watch via jadepaw-bus）。
- **D-07:** Mid-session swap 在当前 turn 完成后生效。检测到 skill change 时设置 `pending_skill_change` flag，在 next turn 顶部原子交换 `messages[0]` 并重建 tool list。不中断 in-flight LLM 调用 —— 避免上下文不一致。
- **D-08:** 验证失败拒绝并保留当前 skill。新 skill 在 staging area 解析/校验，全部通过后原子 swap。正在使用的 skill 不受影响。

### Skill Storage & Discovery
- **D-09:** 文件主存储 + SQLite 索引缓存。SKILL.md 存放于 `~/.jadepaw/skills/<tenant_id>/<skill_name>/SKILL.md` 作为 source of truth。启动时 walkdir 扫描全部目录，提取 YAML frontmatter 元数据写入 SQLite `skill_index` 表（名称、描述、版本、tools 列表、文件路径）。API `list_skills` 走 DB 索引查询（O(log n)），`load_skill` 直接读文件（source of truth）。DB 索引可丢弃 —— 删除后重新扫描即重建。
- **D-10:** 多租户目录隔离。`~/.jadepaw/skills/global/` 存放内置技能，`~/.jadepaw/skills/<tenant_id>/` 存放租户私有技能。租户目录优先（同名可覆盖全局）。路径校验复用 Phase 2 的 canonicalize + prefix check 基础设施。

### State Management
- **D-11:** Skill 运行时状态在内存中管理。`Arc<DashMap<TenantId, SkillState>>` 模式（对齐 InstancePool/ToolRegistry 的并发架构）。重启时从文件系统重新加载。Phase 7+ 集群模式加 DB-backed skill state（`SkillRepository` trait extends SessionRepository pattern）。

### Claude's Discretion
No areas were deferred to Claude — all decisions were user-directed.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Codebase (Phase 3-5 output)
- `crates/jadepaw-agent/src/lib.rs` — `run_agent()` entry point (line 66-171): system prompt building at line 99-108, `AgentRequest.context` at line 77, `ToolRegistry` integration at line 99. This is the PRIMARY integration surface for SkillManager.
- `crates/jadepaw-agent/src/llm.rs` — `build_initial_messages()` (injects context into user message), `stream_llm_response()`, `build_system_prompt_with_tools()`, `REACT_SYSTEM_PROMPT` constant. Skill context injection replaces the current `context: Option<String>` approach.
- `crates/jadepaw-agent/src/loop.rs` — `react_loop()`: the ReAct turn loop where per-turn Late-Binding system prompt rebuild happens. `messages: Vec<ChatCompletionRequestMessage>` at turn boundaries.
- `crates/jadepaw-agent/src/tool_registry.rs` — `ToolRegistry` with DashMap, `register()` (needs panic→Result migration per D-04), `list_tools()`. Skill tool validation target.
- `crates/jadepaw-core/src/agent_types.rs` — `AgentRequest` (line 25: `context: Option<String>` field — needs `skills: Vec<SkillId>` extension), `ReActStep` enum, `AgentResponse`.
- `crates/jadepaw-core/src/lib.rs` — `SkillId` type referenced in doc comments (line 8). May need actual type definition in Phase 6.
- `crates/jadepaw-skill/src/lib.rs` — CURRENT PLACEHOLDER: crate exists with doc comments only, depends on jadepaw-core + jadepaw-wasm + jadepaw-agent. MAIN IMPLEMENTATION TARGET.

### Phase 2 Output (security foundation)
- `crates/jadepaw-wasm/src/session.rs` — `SessionState` (session_id, tenant_id, capabilities). Skill loading is per-tenant.
- `crates/jadepaw-wasm/src/pool.rs` — `InstancePool` with Arc + Semaphore + DashMap. SkillRegistry follows same concurrency pattern.
- `crates/jadepaw-core/src/capabilities.rs` — `InstanceCapabilities`. Skills declare capabilities via `can_exec_tools`.

### Phase 5 Output (persistence)
- `crates/jadepaw-db/` — `SessionRepository` trait + SQLite impl. `SkillRepository` trait follows the same dual-key (`skill_id + tenant_id`) isolation pattern.
- `crates/jadepaw-agent/src/window.rs` — `compress_context()`: retains `messages[0]` (system prompt) — ensures skill instructions survive compression.

### Prior Phase Context
- `.planning/phases/04-tool-system/04-CONTEXT.md` — ToolRegistry dispatch, MCP-compatible wire format. D-02a: Skills declare tool requirements by ToolId; ToolRegistry validates at skill load time.
- `.planning/phases/05-session-memory/05-CONTEXT.md` — SessionRepository dual-key pattern, SQLite WAL mode, context compression. D-09: tenant_id on every session.
- `.planning/phases/03-agent-runtime/03-CONTEXT.md` — ReAct loop architecture, GuestExports trait, system prompt structure.

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` §Skill System — SKILL-01 (declarative format), SKILL-02 (hot loading/swapping)
- `.planning/ROADMAP.md` §Phase 6 — Phase goal, 5 success criteria, depends on Phase 3 + Phase 4
- `.planning/PROJECT.md` — Skill = data-driven configuration (not compiled Wasm), Agent Skills open standard

### Architecture & Design
- `docs/jadepaw_discussion.md` — Skill system design, Wasm isolation model
- `.planning/notes/mvp-core-decisions.md` — MVP core decisions, Skill as natural language program

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `jadepaw-skill` crate — placeholder with dependency graph already wired (jadepaw-core + jadepaw-wasm + jadepaw-agent). Ready to receive: parser module, SkillManager, SkillRegistry, SkillRepository trait.
- `jadepaw-agent` crate — `run_agent()` system prompt at lines 99-108 is the exact injection point. `AgentRequest.context: Option<String>` (line 77) extends to carry skill references. `ToolRegistry` DashMap pattern directly maps to SkillRegistry.
- `jadepaw-core` crate — `InstanceCapabilities.can_exec_tools` is the capability gate for skill-declared tools. `ToolId` type ready for skill tool references.
- `jadepaw-db` crate — `SessionRepository` trait pattern (dual-key isolation, SQLx `query!` for normalized columns, JSON blob for content) applies directly to `SkillRepository`.
- Workspace `Cargo.toml` — `serde`, `serde_json`, `serde_yaml` (0.9+) already available. `DashMap` already in dep tree. No new crate dependencies beyond `walkdir` (directory scanning).

### Established Patterns
- **Types in core, impl downstream**: `SkillManifest`, `SkillId`, `SkillValidationError` in jadepaw-core. `SkillManager`, `SkillRegistry`, `SkillLoader` in jadepaw-skill. `SkillRepository` trait in jadepaw-db. Gateway endpoints in jadepaw-server.
- **Arc<DashMap<K, V>> for concurrent read-heavy state**: InstancePool tracks sessions; ToolRegistry tracks tools; SkillRegistry tracks loaded skills. Same lock-free read pattern.
- **Additive-only interfaces**: Skill system extends `AgentRequest` and system prompt building additively — no existing API is broken.
- **Capability-gated before I/O**: Skill-declared tools still go through `SessionState.can_call_tool()` at dispatch time. Skills declare intent; capabilities enforce authority.
- **Dual-key isolation**: `(skill_id, tenant_id)` on all skill operations — same pattern as Phase 5's `(session_id, tenant_id)`.

### Integration Points
- Phase 7 (Web Chat UI): Skill loading API endpoints consumed by chat interface. Active skill list displayed in chat header.
- Phase 8 (Skill Management UI): Skill CRUD, list, load/unload all go through Phase 6's REST API. DB index enables search/filter.
- Phase 9 (Observability): Skill load/unload events as tracing spans. Active skill gauge in Prometheus metrics.

</code_context>

<specifics>
## Specific Ideas

- **SKILL.md YAML frontmatter fields (D-01):** `name:`, `description:` (required), `tools:` (array of ToolId strings), `constraints:` (natural language string), `version:` (semver string), `author:` (string). `metadata:` map (standard field) preserved for arbitrary key-value passthrough.
- **XML injection format (D-02):** `<skill_instructions>` root wrapping `<skill name="..." version="..." priority="N">` per skill, with raw Markdown body inside as CDATA or escaped text. Resolves to structured block in system prompt.
- **Skill directory layout (D-09):** `~/.jadepaw/skills/<tenant_id>/<skill_name>/SKILL.md`. Single-file per skill. Directory name = skill canonical name (kebab-case). YAML frontmatter `name` must match directory name.
- **API surface:** `POST /skills/load` (tenant_id, skill_name), `POST /skills/unload` (tenant_id, skill_name), `GET /skills/list` (?tenant_id), `GET /skills/inspect/{name}`. Responses are JSON. Errors use structured `SkillValidationError`.
- **`SkillManager` in jadepaw-skill:** Central coordinator — owns `SkillRegistry` (DashMap), `SkillLoader` (file→parsed struct), `SkillIndex` (DB cache sync). Exposes `load()`, `unload()`, `list()`, `merge_active()` -> merged system prompt block + tool list.
- **Turn-boundary swap protocol (D-07):** `SkillManager` exposes `check_pending_swap() -> Option<SkillSwap>`. Called at top of each ReAct turn. On `Some`, `react_loop` replaces `messages[0]`, rebuilds tool list, clears pending flag.
- **Walkdir scanning at startup:** `SkillLoader::scan_all()` runs once at server start. Multi-threaded YAML parse per file. Failures logged individually (one broken skill doesn't block others). Scan results → `SkillIndex::sync()` bulk upsert into SQLite.
- **Tool declaration union (D-04):** `ToolRegistry::register()` signature change: `fn register(&self, tool: Arc<dyn Tool>, priority: u8) -> Result<(), ToolConflictError>`. Conflict = same name, different schema, equal priority → `Err(DuplicateTool { name, existing_priority, new_priority })`.

</specifics>

<deferred>
## Deferred Ideas

- **交互式 Skill 创建 (SKILL-03):** 对话引导 → 意图提取 → 草稿生成 → Wasm 沙箱预览 → 迭代。v2 的 "aha moment" 功能。
- **Skill 版本管理和持续迭代 (SKILL-04):** Git-based 版本历史，diff，回滚。Phase 6 的 `version` 字段为此时预留。
- **Skill 组合为工作流 DAG (SKILL-05):** 多 skill 编排，通过宿主消息总线传递。v2。
- **文件监听自动重载 (notify crate):** Phase 6 用 API 驱动。Hybrid API+Watch 方案（jadepaw-bus 为脊柱）推迟到 Phase 7+。
- **Git-based Skill 分发/市场 (UI-04):** 文件系统设计已兼容 —— `git clone` 到技能目录即可自动索引。完整市场 UI 在 v2。
- **Per-skill token budget:** 每条 skill 的指令占用 token 预算管理。当前 context window 65% 压缩阈值已覆盖全局。
- **Skill 间显式冲突解决 DSL:** XML block 中的 `<conflict resolution>` 语义标记。当前用 priority-based override 覆盖基本需求。

</deferred>

---

*Phase: 6-Skill System*
*Context gathered: 2026-06-05*