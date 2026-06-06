---
phase: 06-skill-system
fixed_at: 2026-06-06T00:00:00Z
review_path: .planning/phases/06-skill-system/06-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 0
skipped: 9
status: none_fixed
---

# Phase 06: Code Review Fix Report

**Fixed at:** 2026-06-06T00:00:00Z
**Source review:** .planning/phases/06-skill-system/06-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 9 (4 critical + 5 warning)
- Fixed: 0
- Skipped: 9

## Fixed Issues

None -- all findings were already resolved by prior fix commits.

## Skipped Issues

All in-scope findings were already fixed in prior iterations. The current codebase (HEAD: `9247c57`) contains all fixes. No additional fixes are needed.

### CR-01: SkillRegistry::insert() does not deduplicate by name

**File:** `crates/jadepaw-skill/src/registry.rs`
**Reason:** Already fixed in commit `f9cc4e8` (fix(06): CR-01 add skill name dedup before registry insertion). Current code at `crates/jadepaw-skill/src/manager.rs:129` calls `self.registry.remove(tenant_id, skill_name)` before `self.registry.insert()`.

### CR-02: skill_index table schema does not enforce dual-key isolation

**File:** `crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql`
**Reason:** Already fixed in commit `9c7a358` (fix(06): CR-02 change skill_index PK to composite (skill_id, tenant_id)). Migration file line 16 now has `PRIMARY KEY (skill_id, tenant_id)`.

### CR-03: build_index_record() always generates a new SkillId

**File:** `crates/jadepaw-skill/src/index.rs`
**Reason:** Already fixed in commit `a4243fe` (fix(06): CR-03 derive stable SkillId from tenant_id + skill_name). Current code at lines 147-155 derives a stable `SkillId` from tenant_id + skill name using UUID v5.

### CR-04: inspect_skill API constructs filesystem path from unvalidated user input

**File:** `crates/jadepaw-server/src/routes/skills.rs`
**Reason:** Already fixed in commit `3889586` (fix(06): CR-04 validate skill_name before filesystem access in inspect_skill). Current code at lines 264-275 validates skill_name BEFORE any filesystem access.

### WR-01: Skill swap replaces messages[0] with just the XML skill block

**File:** `crates/jadepaw-agent/src/loop.rs`
**Reason:** Already fixed in commit `b1756f0` (fix(06): WR-01 reconstruct full system prompt on skill swap). Current code at lines 206-224 reconstructs the full augmented system prompt (base + skill block + tools) before assigning to `messages[0]`.

### WR-02: load_skill and unload_skill API handlers return BAD_REQUEST for all error types

**File:** `crates/jadepaw-server/src/routes/skills.rs`
**Reason:** Already fixed in commit `6ad85a2` (fix(06): WR-02 differentiate validation errors from internal errors in API). Current code at lines 196-210 has `classify_skill_error_status()` that returns 400 for client errors and 500 for server errors.

### WR-03: ToolRegistry::call_tool() uses session.store().data() which may return stale capability state

**File:** `crates/jadepaw-agent/src/tool_registry.rs`
**Reason:** Already fixed in commit `95ba056` (fix(06): WR-03 document coupling between SkillManager and SessionState capabilities). Current code at `crates/jadepaw-skill/src/manager.rs:71-77` documents the coupling between `SkillManager::load()`/`unload()` and `SessionState` capability updates.

### WR-04: llm.rs build_skill_augmented_prompt has unreachable branch in empty-skill case

**File:** `crates/jadepaw-agent/src/llm.rs`
**Reason:** Informational only -- the review explicitly states "No functional impact; informational only. The code is correct but slightly wasteful." The `tool_descriptions` vector is computed even when tools are empty, but this is an allocation optimization, not a bug. No fix needed.

### WR-05: extract_host_from_url returns empty string for edge-case URLs

**File:** `crates/jadepaw-core/src/tool.rs`
**Reason:** Already fixed in commit `9247c57` (fix(06): WR-05 deny http_request with empty host to prevent whitelist bypass). Current code at `crates/jadepaw-agent/src/tool_registry.rs:238-245` has an explicit `host.is_empty()` check that denies requests when no valid host can be extracted.

---

_Fixed: 2026-06-06T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_