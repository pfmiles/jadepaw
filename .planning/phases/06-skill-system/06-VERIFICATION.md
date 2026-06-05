---
phase: 06-skill-system
verified: 2026-06-06T01:30:00Z
status: human_needed
score: 5/5 must-haves verified (roadmap SC) + 24/24 plan truths verified
overrides_applied: 0

human_verification:
  - test: "创建 SKILL.md 文件放入 skills 目录，通过 POST /skills/load 加载，发送对话请求，确认 Agent 行为发生变化"
    expected: "Agent 的回复反映出 SKILL.md 中定义的技能行为（如 code-reviewer 风格审查代码）"
    why_human: "Agent 行为变化是端到端语义结果，无法通过 grep 或 cargo test 自动验证"
  - test: "会话中途通过 POST /skills/load 切换技能，继续同一会话发送消息，确认 Agent 行为在新回合切换"
    expected: "下一个 ReAct turn 开始后，Agent 行为切换到新技能（旧技能指令不再生效）"
    why_human: "中会话切换涉及真实的 LLM 调用和 ReAct 循环状态，无法静态验证"
  - test: "通过 POST /skills/unload 卸载技能后继续对话，确认 Agent 恢复默认行为"
    expected: "Agent 行为回退到无技能时的基础 ReAct 模式"
    why_human: "行为回退是端到端语义结果，需实际体验对话质量变化"
  - test: "提交格式错误的 SKILL.md（无效 YAML、缺少 name 字段、name 与目录名不匹配），观察 API 返回的错误信息"
    expected: "API 返回 400 状态码，错误消息清晰指出具体问题（哪个字段、什么规则违反）"
    why_human: "错误消息的可读性、用户体验需人工判断"
  - test: "同时加载多个 SKILL.md 技能，确认 Agent 系统提示中正确合并了工具声明和指令"
    expected: "系统提示中包含所有已加载技能的 <skill_instructions> XML 块，工具列表去重合并"
    why_human: "多技能合并的正确性和优先级排序需通过实际 LLM 调用观察行为"
  - test: "删除 SQLite 数据库文件后重启服务，确认索引重新构建正确"
    expected: "walkdir 扫描重新发现所有 SKILL.md 文件并重建 skill_index 表"
    why_human: "幂等性重建的正确性涉及数据库状态和文件系统状态的一致性，需端到端验证"
---

# Phase 6: Skill System Verification Report

**Phase Goal:** Users can define agent behaviors through declarative SKILL.md files, load them at runtime, and swap between different skills without restarting the agent.
**Verified:** 2026-06-06T01:30:00Z
**Status:** human_needed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths (Roadmap Success Criteria)

| #   | Truth   | Status     | Evidence       |
| --- | ------- | ---------- | -------------- |
| 1   | SKILL.md file placed in skills directory, agent adopts behavior on next invocation | ✓ VERIFIED | `SkillManager::load()` reads from disk, parses, inserts into registry, calls `merge_active()`; `run_agent()` calls `build_skill_augmented_prompt()` which injects XML `<skill_instructions>` between base prompt and tool descriptions; `react_loop()` receives augmented system prompt at `messages[0]` |
| 2   | Mid-session skill swap changes agent behavior without restart | ✓ VERIFIED | `SkillManager::load()` sets `pending_swap` on registry; `react_loop()` checks `check_pending_swap()` at top of each turn (after compression, before LLM call); `messages[0]` replaced atomically with new system prompt |
| 3   | Unloading a skill reverts agent to default behavior | ✓ VERIFIED | `SkillManager::unload()` removes from registry via `registry.remove()`, rebuilds context block via `merge_active()`, sets new `pending_swap`; if no skills remain, `merge_active()` returns empty string |
| 4   | Invalid YAML frontmatter rejected with clear error message | ✓ VERIFIED | `parse_skill_file()` in `parser.rs` validates YAML via gray_matter with validation-before-deserialization; returns `SkillValidationError::ParseError` with file path and line info; all 7 error variants have descriptive Display messages |
| 5   | Multiple skills loaded simultaneously merge tool declarations and instruction contexts | ✓ VERIFIED | `merge_active()` collects all active skills sorted by priority, calls `build_skill_context_block()` to produce `<skill_instructions>` XML with per-skill `<skill>` tags; tool names union-deduplicated across all skills |

**Score:** 5/5 roadmap success criteria verified

### Plan Must-Have Truths (Plan 01)

| #   | Truth   | Status     | Evidence       |
| --- | ------- | ---------- | -------------- |
| 1   | SkillManifest struct exists with all D-01 fields | ✓ VERIFIED | `crates/jadepaw-core/src/skill_types.rs` line 58: struct with name, description, tools, constraints, version, author, metadata, source_path |
| 2   | SKILL.md files with valid YAML frontmatter parse into SkillManifest + Markdown body | ✓ VERIFIED | `parse_skill_file()` at `crates/jadepaw-skill/src/parser.rs` line 49; 67 tests pass including fixture-based parsing tests |
| 3   | Invalid YAML frontmatter rejected with field-level SkillValidationError | ✓ VERIFIED | 7 variants: MissingFrontmatter, MissingField, InvalidName, FieldTooLong, NameDirectoryMismatch, ToolNotFound, ParseError -- all with Display and Error impls |
| 4   | Skill name validation enforces kebab-case, 1-64 chars | ✓ VERIFIED | `validate_skill_name()` in `validation.rs` line 19: ASCII lowercase, digits, hyphens only; no leading/trailing/consecutive hyphens; 1-64 length |
| 5   | Name-directory mismatch detected and rejected | ✓ VERIFIED | `parse_skill_file()` line 103 compares name against dir_name; returns `NameDirectoryMismatch` variant |
| 6   | Missing required fields (name, description) rejected at parse time | ✓ VERIFIED | `parse_skill_file()` extracts name and description from Pod via `extract_required_string()` before SkillManifest construction; returns `MissingField` variant |

### Plan Must-Have Truths (Plan 02)

| #   | Truth   | Status     | Evidence       |
| --- | ------- | ---------- | -------------- |
| 1   | SkillRegistry stores per-tenant skills with DashMap | ✓ VERIFIED | `registry.rs` line 24: `states: DashMap<TenantId, ActiveSkillState>`; all methods (insert, remove, get_active, set_pending_swap, take_pending_swap) use DashMap lock-free reads |
| 2   | SkillManager exposes load, unload, list, merge_active, check_pending_swap APIs | ✓ VERIFIED | `manager.rs` lines 70, 150, 183, 210, 215: all public methods verified; load does file read -> parse -> tool validation -> registry insert -> pending swap; unload does remove -> merge -> pending swap |
| 3   | merge_active returns XML skill_instructions sorted by priority + merged tool list | ✓ VERIFIED | `merge_active()` calls `build_skill_context_block()` which formats `<skill_instructions>` with per-skill `<skill>` tags; tools union-deduplicated |
| 4   | Skill injection uses late-binding per-turn rebuild (D-03) | ✓ VERIFIED | `run_agent()` calls `merge_active()` + `build_skill_augmented_prompt()` before entering `react_loop`; `messages[0]` is built fresh each turn |
| 5   | Mid-session swap applies atomically at turn boundary (D-07) | ✓ VERIFIED | `react_loop()` line 199-210: `check_pending_swap()` called after compression, before `stream_llm_response`; `SkillSwap` consumed via DashMap atomic take |
| 6   | Unload removes skill and agent reverts to default | ✓ VERIFIED | `unload()` removes from registry, rebuilds via `merge_active()`, sets empty block as pending swap when no skills remain |
| 7   | ToolRegistry::register() returns Result instead of panicking (D-04) | ✓ VERIFIED | `tool_registry.rs` line 111: returns `Result<ToolId, ToolConflictError>`; `DuplicateTool` and `SchemaMismatch` variants with Display+Error impls |
| 8   | Multiple skills merge tool declarations with priority-based conflict resolution (D-04) | ✓ VERIFIED | `merge_active()` collects tool names from all active skills, deduplicates; `ToolRegistry::register()` with priority handles same-tool conflicts |
| 9   | Skill loading validates tool availability (D-05) | ✓ VERIFIED | `SkillManager::load()` uses `ToolLookup::lookup_by_name()` to validate each tool; returns `ToolNotFound` if any tool unknown |
| 10  | Validation failure retains current active skills (D-08) | ✓ VERIFIED | `SkillManager::load()` does all validation BEFORE calling `registry.insert()`; on any error, returns early without modifying registry |

### Plan Must-Have Truths (Plan 03)

| #   | Truth   | Status     | Evidence       |
| --- | ------- | ---------- | -------------- |
| 1   | SkillRepository trait with dual-key isolation | ✓ VERIFIED | `skill_repository.rs` line 33: `async fn sync_index`, `list_by_tenant`, `get_by_name`, `delete` all require both skill_id and tenant_id |
| 2   | SqliteSkillRepo persists in SQLite using shared pool | ✓ VERIFIED | `sqlite_skill_repo.rs` line 36: struct with `pool: SqlitePool`; constructor takes pool (no separate pool creation) |
| 3   | Startup walkdir scan discovers SKILL.md files and syncs to SQLite | ✓ VERIFIED | `main.rs` lines 62-79: `SkillLoader::scan_all()` via `spawn_blocking`, `SkillIndex::sync()` parses and bulk-upserts; invalid files logged and skipped |
| 4   | POST /skills/load returns 200 or structured error | ✓ VERIFIED | `routes/skills.rs` line 112: calls `skill_manager.load()`, returns `StatusCode::OK` or `StatusCode::BAD_REQUEST` with error JSON |
| 5   | POST /skills/unload returns 200 | ✓ VERIFIED | `routes/skills.rs` line 152: calls `skill_manager.unload()`, returns OK |
| 6   | GET /skills/list returns indexed skills per tenant | ✓ VERIFIED | `routes/skills.rs` line 197: calls `skill_repo.list_by_tenant()`, returns JSON array |
| 7   | GET /skills/inspect/{name} returns full SKILL.md content | ✓ VERIFIED | `routes/skills.rs` line 235: reads from filesystem (source of truth per D-09), returns manifest + body |
| 8   | Multi-tenant directory isolation (D-10) | ✓ VERIFIED | `SkillManager::load()` builds path `skills_root/<tenant_id>/<skill_name>/SKILL.md`; `SkillLoader` distinguishes global vs tenant dirs |
| 9   | SkillIndex sync is idempotent (D-09) | ✓ VERIFIED | `index.rs` sync uses INSERT OR REPLACE pattern; existing records overwritten; invalid files skipped |

### Deferred Items

No deferred items. All Phase 6 must-haves are satisfied in this phase.

### Required Artifacts

| Artifact | Expected    | Status | Details |
| -------- | ----------- | ------ | ------- |
| `crates/jadepaw-core/src/skill_types.rs` | SkillId, SkillManifest, SkillValidationError | ✓ VERIFIED | 232 lines; all 3 types with derive/impls; 7 error variants with Display+Error |
| `crates/jadepaw-skill/src/parser.rs` | parse_skill_file with gray_matter | ✓ VERIFIED | 634 lines; YAML frontmatter parsing with validation-before-deserialization |
| `crates/jadepaw-skill/src/validation.rs` | validate_skill_name | ✓ VERIFIED | 247 lines; kebab-case, 1-64 chars, full Agent Skills spec compliance |
| `crates/jadepaw-skill/src/manifest.rs` | SkillManifest re-export | ✓ VERIFIED | 7 lines; re-exports from jadepaw-core |
| `crates/jadepaw-skill/src/registry.rs` | DashMap concurrent registry | ✓ VERIFIED | 284 lines; ActiveSkillState, LoadedSkill, SkillSwap types; lock-free concurrent access |
| `crates/jadepaw-skill/src/manager.rs` | SkillManager coordinator | ✓ VERIFIED | 402 lines; load/unload/merge_active/check_pending_swap APIs; ToolLookup trait for circular dep resolution |
| `crates/jadepaw-skill/src/injector.rs` | XML skill_instructions builder | ✓ VERIFIED | 149 lines; build_skill_context_block with priority-sorted XML tags |
| `crates/jadepaw-skill/src/loader.rs` | walkdir scanner | ✓ VERIFIED | 249 lines; scan_all with tenant directory awareness |
| `crates/jadepaw-skill/src/index.rs` | SQLite cache sync | ✓ VERIFIED | 227 lines; parse+sync with graceful error skipping |
| `crates/jadepaw-agent/src/llm.rs` | build_skill_augmented_prompt | ✓ VERIFIED | Line 110; 4-way combination handler (skills +/- tools) |
| `crates/jadepaw-agent/src/loop.rs` | pending_skill_change at turn boundary | ✓ VERIFIED | Line 199; check_pending_swap after compression, before LLM call |
| `crates/jadepaw-agent/src/lib.rs` | SkillManager threading through run_agent | ✓ VERIFIED | Line 74; skill-aware system prompt construction; passed to react_loop |
| `crates/jadepaw-agent/src/tool_registry.rs` | register() -> Result migration | ✓ VERIFIED | Line 111; ToolConflictError with DuplicateTool + SchemaMismatch |
| `crates/jadepaw-core/src/agent_types.rs` | skills: Vec<String> field | ✓ VERIFIED | Line 44; #[serde(default)] |
| `crates/jadepaw-core/src/tool.rs` | ToolLookup trait | ✓ VERIFIED | Line 37; breaks circular dependency |
| `crates/jadepaw-db/src/skill_repository.rs` | SkillRepository trait | ✓ VERIFIED | 65 lines; sync_index/list_by_tenant/get_by_name/delete |
| `crates/jadepaw-db/src/skill_models.rs` | SkillIndexRecord, SkillIndexSummary | ✓ VERIFIED | 59 lines; dual-key data models |
| `crates/jadepaw-db/src/sqlite_skill_repo.rs` | SQLite impl | ✓ VERIFIED | 203 lines; UUID BLOB binding, dual-key isolation |
| `crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql` | skill_index DDL | ✓ VERIFIED | 22 lines; CREATE TABLE + 2 composite indexes |
| `crates/jadepaw-server/src/routes/skills.rs` | REST endpoints | ✓ VERIFIED | 280 lines; 4 axum handlers, SkillApiState, structured error responses |
| `crates/jadepaw-server/src/main.rs` | Startup sequence | ✓ VERIFIED | 111 lines; DB pool, migration, walkdir scan, index sync, axum serve |

### Key Link Verification

| From | To  | Via | Status | Details |
| ---- | --- | --- | ------ | ------- |
| `parser.rs` | `validation.rs` | `validate_skill_name` call | ✓ VERIFIED | Line 97: `validation::validate_skill_name(&name)?;` |
| `parser.rs` | `manifest.rs` | `SkillManifest` construction | ✓ VERIFIED | Uses `crate::manifest::SkillManifest` via re-export |
| `parser.rs` | `skill_types.rs` | `SkillValidationError::` variants | ✓ VERIFIED | Returns `SkillValidationError::MissingFrontmatter`, `::MissingField`, etc. |
| `manager.rs` | `injector.rs` | `build_skill_context_block` | ✓ VERIFIED | Line 189: `let skill_block = build_skill_context_block(&active);` |
| `manager.rs` | `tool_registry.rs` | `ToolLookup` trait | ✓ VERIFIED | Uses `ToolLookup::lookup_by_name()` via trait (not direct reference) |
| `loop.rs` | `manager.rs` | `check_pending_swap` | ✓ VERIFIED | Line 200: `sm.check_pending_swap(tenant_id)` |
| `lib.rs` (agent) | `manager.rs` | `merge_active` | ✓ VERIFIED | Line 108: `sm.merge_active(tenant_id)` |
| `loader.rs` | `index.rs` | `SkillIndex::sync` | ✓ VERIFIED | `main.rs` line 79: `skill_index.sync(&scan_entries)` |
| `skills.rs` (routes) | `manager.rs` | `skill_manager.load/unload/merge_active` | ✓ VERIFIED | Lines 127, 165, 241: direct calls to `skill_manager` methods |
| `skills.rs` (routes) | `skill_repository.rs` | `skill_repo.list_by_tenant` | ✓ VERIFIED | Line 201: `state.skill_repo.list_by_tenant(query.tenant_id)` |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| `SkillManager::load()` | `manifest`, `body` | `tokio::fs::read_to_string` -> `parse_skill_file` | Yes -- file read + parsing pipeline | ✓ FLOWING |
| `SkillManager::merge_active()` | `active` | `registry.get_active(tenant_id)` | Yes -- DashMap reads real inserted skills | ✓ FLOWING |
| `build_skill_augmented_prompt()` | `skill_context_block`, `tools` | `SkillManager::merge_active()` + `ToolRegistry::list_tools()` | Yes -- formatted XML from real manifest data | ✓ FLOWING |
| `react_loop swap` | `skill_swap.new_system_prompt` | `SkillManager::check_pending_swap()` | Yes -- pre-built augmented prompt from SkillSwap | ✓ FLOWING |
| `SkillIndex::sync()` | `records` | `tokio::fs::read_to_string` -> `parse_skill_file` -> `build_index_record` | Yes -- file reads + SQLite upsert | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Workspace compilation | `cargo check -p jadepaw-core -p jadepaw-skill -p jadepaw-agent -p jadepaw-db -p jadepaw-server` | 0 errors (2 minor unused import warnings) | ✓ PASS |
| Core tests | `cargo test -p jadepaw-core` | 0 tests (no test target in core) | ? SKIP |
| Skill tests | `cargo test -p jadepaw-skill` | 67 passed, 0 failed | ✓ PASS |
| DB tests | `cargo test -p jadepaw-db` | 0 tests (no test target in db) | ? SKIP |
| Agent lib tests | `cargo test -p jadepaw-agent --lib` | 33 passed, 0 failed | ✓ PASS |

### Probe Execution

| Probe | Command | Result | Status |
| ----- | ------- | ------ | ------ |
| N/A | No probes declared for this phase | -- | ? SKIP |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ---------- | ----------- | ------ | -------- |
| SKILL-01 | 06-01, 06-03 | 声明式 Skill 格式：采用 Agent Skills 开放标准 (SKILL.md: YAML frontmatter + Markdown 指令)，可版本控制 | ✓ SATISFIED | `parse_skill_file()` parses YAML frontmatter + Markdown body; `SkillManifest` has version field; `SkillIndex::sync()` caches metadata in SQLite |
| SKILL-02 | 06-02, 06-03 | Skill 热加载和运行时切换，Skill 作为 Agent 行为的持久化配置注入执行上下文 | ✓ SATISFIED | `SkillManager::load()` loads at runtime; `react_loop()` applies mid-session swap atomically; `build_skill_augmented_prompt()` injects into system prompt; `AgentRequest.skills` field for server-initiated loading |

**All Phase 6 requirements (SKILL-01, SKILL-02) satisfied.** No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| `jadepaw-skill/src/loader.rs` | 27 | Unused import `Path` | Warning | Compilation warning only, no functional impact |
| `jadepaw-server/src/routes/skills.rs` | 23 | Unused import `SkillManifest` | Warning | Compilation warning only, no functional impact |

No blocker-level anti-patterns (TBD, FIXME, XXX) found. No stubs, no empty returns, no hardcoded fake data in production code.

### Human Verification Required

#### 1. End-to-End Skill Loading and Behavior Change

**Test:** Create a SKILL.md file (e.g., `code-reviewer` skill with instructions to review code), place it in `~/.jadepaw/skills/<tenant_id>/code-reviewer/SKILL.md`. Start the server. Send POST /skills/load with `{tenant_id, skill_name: "code-reviewer"}`. Then send a chat message asking about code -- verify the agent's response reflects the skill's behavior.

**Expected:** Agent responds in code-reviewer style, follows the instructions in the SKILL.md body.

**Why human:** Agent behavior change is a semantic end-to-end outcome requiring actual LLM interaction. Static analysis can verify the piping exists, not that the LLM actually follows the skill's instructions.

#### 2. Mid-Session Skill Swap

**Test:** Start a conversation with one skill loaded (e.g., "code-reviewer"). Mid-conversation, POST /skills/load a different skill (e.g., "data-analyst"). Continue the conversation on the same session. Verify the agent's next response reflects the new skill.

**Expected:** The next ReAct turn starts with the new skill's system prompt -- agent behavior switches to data-analyst style without restarting.

**Why human:** Mid-session swap involves real ReAct loop state, LLM context window dynamics, and timing-dependent behavior that cannot be verified from code alone.

#### 3. Skill Unload -- Behavior Reversion

**Test:** With a skill loaded and active in a conversation, POST /skills/unload. Continue the conversation. Verify the agent reverts to default behavior.

**Expected:** Agent stops following skill-specific instructions and returns to base ReAct behavior.

**Why human:** Behavior reversion quality depends on LLM interpretation of the unified system prompt. Static analysis confirms the swap mechanism exists, not the quality of reversion.

#### 4. Error Message Quality for Invalid SKILL.md

**Test:** Submit malformed SKILL.md files (invalid YAML syntax, missing name field, name contains uppercase letters, description > 1024 chars, name != directory name). Observe the API error responses.

**Expected:** Each error returns HTTP 400 with a clear, human-readable error message (Chinese or English) that identifies the specific field and the rule violated.

**Why human:** Error message readability, localization quality, and UX clarity are subjective assessments.

#### 5. Multi-Skill Merge Behavior

**Test:** Create two SKILL.md files (e.g., "code-reviewer" and "security-auditor"), load both simultaneously. Send a message that triggers both skill domains. Verify the agent incorporates instructions from both skills.

**Expected:** System prompt contains `<skill_instructions>` with `<skill>` tags for both skills, sorted by priority. Agent behavior reflects merged skill instructions.

**Why human:** Multi-skill instruction merging and the LLM's ability to follow combined instructions are semantic behaviors requiring real LLM interaction.

#### 6. Startup Scan Idempotency

**Test:** Delete the SQLite database file (or `skill_index` rows). Restart the server. Verify the skill_index table is repopulated correctly.

**Expected:** Server starts without errors, walkdir scan re-discovers all SKILL.md files, and `GET /skills/list` returns the same skills as before.

**Why human:** Database state + filesystem state consistency is an integration concern requiring real server restart and state inspection.

### Gaps Summary

No implementation gaps found. All 5 roadmap success criteria, all 24 plan must-have truths, all 21 artifacts, and all 10 key links are verified against the codebase. Both requirements (SKILL-01, SKILL-02) are fully satisfied.

Two minor code quality warnings exist (unused imports in `loader.rs` and `routes/skills.rs`) but do not affect functionality.

The skill system is fully wired: SKILL.md files are parsed and validated at load time, loaded skills are stored in a concurrent per-tenant registry, their instructions are injected into the agent's system prompt via late-binding per-turn rebuilds, mid-session swaps are handled atomically at turn boundaries, and the entire system is exposed through REST API endpoints served by the axum server with SQLite-backed metadata caching.

6 items require human verification for end-to-end behavioral confirmation.

---

_Verified: 2026-06-06T01:30:00Z_
_Verifier: Claude (gsd-verifier)_