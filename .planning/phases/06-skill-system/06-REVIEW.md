---
phase: 06-skill-system
reviewed: 2026-06-06T00:00:00Z
depth: standard
files_reviewed: 28
files_reviewed_list:
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-agent/src/llm.rs
  - crates/jadepaw-agent/src/loop.rs
  - crates/jadepaw-agent/src/tool_registry.rs
  - crates/jadepaw-agent/tests/agent_loop.rs
  - crates/jadepaw-core/src/agent_types.rs
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-core/src/tool.rs
  - crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql
  - crates/jadepaw-db/src/lib.rs
  - crates/jadepaw-db/src/skill_models.rs
  - crates/jadepaw-db/src/skill_repository.rs
  - crates/jadepaw-db/src/sqlite_skill_repo.rs
  - crates/jadepaw-server/Cargo.toml
  - crates/jadepaw-server/src/main.rs
  - crates/jadepaw-server/src/routes/mod.rs
  - crates/jadepaw-server/src/routes/skills.rs
  - crates/jadepaw-skill/Cargo.toml
  - crates/jadepaw-skill/src/index.rs
  - crates/jadepaw-skill/src/injector.rs
  - crates/jadepaw-skill/src/lib.rs
  - crates/jadepaw-skill/src/loader.rs
  - crates/jadepaw-skill/src/manager.rs
  - crates/jadepaw-skill/src/registry.rs
findings:
  critical: 4
  warning: 5
  info: 4
  total: 13
status: issues_found
---

# Phase 6: Code Review Report

**Reviewed:** 2026-06-06T00:00:00Z
**Depth:** standard
**Files Reviewed:** 28
**Status:** issues_found

## Summary

Reviewed the Phase 6 skill system implementation across 28 source files spanning 6 crates. The architecture is well-structured with clear module boundaries: `jadepaw-core` defines types, `jadepaw-skill` handles parsing/loading/management, `jadepaw-db` provides persistence, `jadepaw-agent` orchestrates the ReAct loop with skill injection, and `jadepaw-server` exposes REST APIs.

Four critical issues were found: (1) `SkillManager::load()` does not check for duplicate skill names before insertion, allowing duplicate entries in the registry; (2) the `skill_index` table uses the wrong primary key for dual-key isolation, creating a data integrity gap where `INSERT OR REPLACE` on `skill_id` alone can silently overwrite cross-tenant records; (3) `build_index_record()` always generates a fresh `SkillId`, making re-syncs produce orphan records rather than updates; (4) the `inspect_skill` API endpoint constructs a filesystem path from unvalidated user input before validation occurs, enabling path traversal attacks.

## Critical Issues

### CR-01: SkillRegistry::insert() does not deduplicate by name, allowing duplicate skill entries

**File:** `crates/jadepaw-skill/src/registry.rs:60-63`
**Issue:** `SkillRegistry::insert()` unconditionally pushes the new skill into `loaded_skills` without checking if a skill with the same name already exists. The doc comment on line 58-59 states: "Skills with the same name are NOT deduplicated here — dedup is the caller's responsibility (SkillManager::load checks before insert)." However, `SkillManager::load()` in `manager.rs:107-117` does NOT check for existing skills before calling `self.registry.insert()`. This means loading the same skill twice will result in duplicate entries in `loaded_skills`, which will:
- Break `merge_active()` by producing duplicate tool names in the system prompt.
- Cause `unload()` via `registry.remove()` to only remove the first duplicate, leaving stale entries.
- Cause `injector::build_skill_context_block()` to emit duplicate skill blocks.

**Fix:** Either add a dedup check in `SkillRegistry::insert()` or add the check in `SkillManager::load()` before line 117. Recommended approach — add the check in `SkillManager::load()` to maintain the documented caller-responsibility contract:

```rust
// In SkillManager::load(), after building the LoadedSkill but before registry.insert():
// Check if a skill with this name is already loaded (avoid duplicates)
let existing = self.registry.get_active(tenant_id);
if let Some(skills) = existing {
    if skills.iter().any(|s| s.manifest.name == skill_name) {
        // Remove the old entry first, then insert the new one
        self.registry.remove(tenant_id, skill_name);
    }
}
```

### CR-02: skill_index table schema does not enforce dual-key isolation at the database level

**File:** `crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql:6-16`
**Issue:** The table schema declares `skill_id BLOB PRIMARY KEY NOT NULL`, making `skill_id` the sole uniqueness constraint. The doc comment in `sqlite_skill_repo.rs:57-59` states: "The ON CONFLICT clause with WHERE tenant_id ensures that a (skill_id, tenant_id) pair uniquely identifies a record — a different tenant_id for the same skill_id is treated as a separate record." However, the actual SQL at `sqlite_skill_repo.rs:62-66` uses plain `INSERT OR REPLACE` without any `ON CONFLICT` clause. Furthermore, even with an `ON CONFLICT`, there is no unique constraint on `(skill_id, tenant_id)` in the schema, so SQLite would not recognize a conflict on those columns.

The practical consequence: if two tenants happen to generate the same `SkillId` (or one tenant uses a UUID that collides with another tenant's), the `INSERT OR REPLACE` would silently overwrite the other tenant's record because the PK is only `skill_id`. This is a cross-tenant data integrity gap.

**Fix:** Create a composite primary key or unique constraint on `(skill_id, tenant_id)`:

```sql
CREATE TABLE IF NOT EXISTS skill_index (
    skill_id    BLOB NOT NULL,
    tenant_id   BLOB NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version     TEXT,
    tools_json  TEXT NOT NULL DEFAULT '[]',
    file_path   TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (skill_id, tenant_id)
);
```

And update `sync_index()` to use `INSERT OR REPLACE INTO skill_index (skill_id, tenant_id, ...)` which will now correctly replace only the row matching both columns.

### CR-03: build_index_record() always generates a new SkillId, making re-syncs create orphan records

**File:** `crates/jadepaw-skill/src/index.rs:139-153`
**Issue:** `build_index_record()` calls `SkillId::new()` every time it is invoked (line 143). `SkillIndex::sync()` calls this during every startup scan. However, `SqliteSkillRepo::sync_index()` uses `INSERT OR REPLACE` which maps `skill_id` to the primary key. Since each sync produces a new `SkillId`, each run creates a brand-new row instead of updating the existing one. Over multiple server restarts, the `skill_index` table accumulates orphan records — old rows with previous `SkillId` values are never cleaned up. Additionally, `delete()` uses `skill_id` as a WHERE clause, so after a re-sync, the API cannot delete the new record using the old ID.

**Fix:** The `SkillFileEntry` should carry a stable skill identity that survives across scans. The simplest fix is to derive the `SkillId` from the tenant_id + skill_name combination (e.g., using UUID v5 with a namespace):

```rust
fn build_index_record(
    manifest: SkillManifest,
    tenant_id: TenantId,
    file_path: String,
) -> SkillIndexRecord {
    // Derive a stable SkillId from tenant_id + skill_name so re-syncs
    // update the same database row instead of creating orphans.
    let namespace = Uuid::parse_str("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap();
    let skill_id = SkillId::from(Uuid::new_v5(&namespace, format!("{}:{}", tenant_id, manifest.name).as_bytes()));
    // ... rest unchanged
}
```

### CR-04: inspect_skill API constructs filesystem path from unvalidated user input before validation

**File:** `crates/jadepaw-server/src/routes/skills.rs:241-245` and `crates/jadepaw-skill/src/parser.rs:49-132`
**Issue:** The `inspect_skill` handler uses the `name` path parameter from the URL to construct a filesystem path at lines 242-245:

```rust
let file_path = skills_root
    .join(query.tenant_id.to_string())
    .join(&name)
    .join("SKILL.md");
```

The `name` is a user-supplied URL path segment. Although `parse_skill_file()` validates the skill name (called at line 261), it validates it AFTER the file has already been read via `tokio::fs::read_to_string(&file_path)` at line 247. A path traversal attack using `../../` in the `name` parameter would cause `tokio::fs::read_to_string` to read an arbitrary file on disk. The subsequent `parse_skill_file()` validation would fail, but by then the file has already been read and its content checked for a file-not-found error path only.

Additionally, the security doc comment at lines 230-233 claims: "The skill_name is validated by parse_skill_file() before filesystem access" — this is factually incorrect; the validation happens after the read.

**Fix:** Validate the skill name BEFORE constructing the filesystem path. Use the same `validate_skill_name()` function from `jadepaw-skill`:

```rust
async fn inspect_skill(
    State(state): State<SkillApiState>,
    Path(name): Path<String>,
    Query(query): Query<ListSkillsQuery>,
) -> Response {
    // Validate skill_name BEFORE any filesystem access (T-06-10)
    if let Err(validation_err) = jadepaw_skill::validate_skill_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(SkillErrorResponse {
                field: "skill_name".to_string(),
                reason: format!("{:?}", validation_err),
            }),
        ).into_response();
    }

    let skills_root = &state.skill_manager.skills_root;
    let file_path = skills_root
        .join(query.tenant_id.to_string())
        .join(&name)
        .join("SKILL.md");
    // ... rest unchanged
}
```

Additionally, `jadepaw_skill::validate_skill_name` should be made public in `lib.rs` (it already is — line 37 of `lib.rs` re-exports it as `pub use validation::validate_skill_name`).

## Warnings

### WR-01: Skill swap replaces messages[0] with just the XML skill block, not the full augmented prompt

**File:** `crates/jadepaw-agent/src/loop.rs:200-210` and `crates/jadepaw-skill/src/manager.rs:122-128`
**Issue:** In `SkillManager::load()` (manager.rs:122-128), the `SkillSwap.new_system_prompt` is set to only the `skill_block` (the XML `<skill_instructions>` block). When `react_loop` applies the swap at line 206-209, it replaces `messages[0]` with this value. However, `messages[0]` is expected to be the full system prompt (base ReAct instructions + skill block + tool descriptions). After a skill swap, the LLM would lose the base ReAct instructions and tool descriptions, receiving only the skill XML block as its system prompt. This would likely cause the LLM to stop following the ReAct format.

**Fix:** In `SkillManager::load()`, build the full augmented prompt (base + skill block + tools) rather than just the skill block. This requires either passing the base prompt and tool definitions to `SkillManager::load()` or having `react_loop` reconstruct the full prompt from the skill block after applying the swap.

```rust
// In SkillManager::load(), replace lines 120-128:
let (skill_block, _tool_names) = self.merge_active(tenant_id);
// Store ONLY the skill block — react_loop will recombine it with the base
// prompt and tool descriptions at swap application time.
let swap = SkillSwap {
    new_system_prompt: skill_block,  // Only the XML block
    merged_tool_list: self.merge_tool_definitions(tenant_id, tool_lookup),
};
```

And in `react_loop` at lines 200-210, reconstruct the full prompt:

```rust
if let Some(skill_swap) = sm.check_pending_swap(tenant_id) {
    let tool_definitions = skill_swap.merged_tool_list;
    let full_prompt = if skill_swap.new_system_prompt.is_empty() {
        if tool_definitions.is_empty() {
            llm::REACT_SYSTEM_PROMPT.to_string()
        } else {
            llm::build_system_prompt_with_tools(llm::REACT_SYSTEM_PROMPT, &tool_definitions)
        }
    } else {
        llm::build_skill_augmented_prompt(
            llm::REACT_SYSTEM_PROMPT,
            &skill_swap.new_system_prompt,
            &tool_definitions,
        )
    };
    messages[0] = ChatCompletionRequestSystemMessage::from(full_prompt).into();
}
```

### WR-02: load_skill and unload_skill API handlers return BAD_REQUEST for all error types, including internal errors

**File:** `crates/jadepaw-server/src/routes/skills.rs:128-145` and `skills.rs:168-185`
**Issue:** Both `load_skill` and `unload_skill` handlers catch errors with `Err(e)` and return `StatusCode::BAD_REQUEST`. However, not all errors are client errors — for example, an IO error reading from disk or a DashMap internal error should be a `500 Internal Server Error`, not a `400 Bad Request`. Returning `400` for infrastructure failures can mislead API consumers into thinking they can fix the request when the issue is server-side.

**Fix:** Differentiate between validation errors (return 400) and runtime/infrastructure errors (return 500):

```rust
Err(e) => {
    tracing::warn!(...);
    let status = match &e {
        SkillValidationError::ParseError { .. }
        | SkillValidationError::MissingField { .. }
        | SkillValidationError::InvalidName { .. }
        | SkillValidationError::FieldTooLong { .. }
        | SkillValidationError::NameDirectoryMismatch { .. }
        | SkillValidationError::ToolNotFound { .. }
        | SkillValidationError::MissingFrontmatter { .. } => StatusCode::BAD_REQUEST,
        // IO errors from reading files are server-side issues
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(SkillErrorResponse { ... })).into_response()
}
```

### WR-03: ToolRegistry::call_tool() uses session.store().data() which may return stale capability state

**File:** `crates/jadepaw-agent/src/tool_registry.rs:215-227`
**Issue:** The capability check accesses `session.store().data()` at line 216, which returns the `SessionState` stored in the Wasm store. However, in the `react_loop`, skill swaps can change the tool set available. The `ToolRegistry` itself is stateless regarding capability, but `SessionState::can_call_tool()` is consulted per-request. If the SessionState's capability set is not updated when skills change, a tool might be incorrectly allowed or denied.

This is partially a design observation rather than a confirmed bug — the `capabilities` module in `jadepaw-core` (not reviewed in depth here) would need to be kept in sync with skill changes. Worth verifying that skill load/unload updates the session capability set.

**Fix:** Document the coupling between `SkillManager::load()`/`unload()` and `SessionState` capability updates, or add a test that verifies tool access is correctly gated before and after skill swaps.

### WR-04: llm.rs build_skill_augmented_prompt has unreachable branch in empty-skill case

**File:** `crates/jadepaw-agent/src/llm.rs:115-152`
**Issue:** In `build_skill_augmented_prompt`, the match arms are:
- `(true, true)` -> return base prompt
- `(true, false)` -> delegate to `build_system_prompt_with_tools`
- `(false, _)` -> build with skills

In the `(false, _)` arm, line 135-136 checks `if tools.is_empty()` again to conditionally add a tools section. But the `(false, _)` match arm means `skill_context_block.is_empty() == false`, so the skill block is always present. The inner `if tools.is_empty()` check at line 135 is correct (don't add an empty tools section), but the `tool_descriptions` vector is still computed on lines 119-133 even when tools are empty, resulting in an unnecessary allocation. Not a bug — just dead computation in the tools-empty path.

No functional impact; informational only. The code is correct but slightly wasteful.

### WR-05: extract_host_from_url returns empty string for edge-case URLs, leading to potential domain whitelist bypass

**File:** `crates/jadepaw-core/src/tool.rs:60-116`
**Issue:** When `extract_host_from_url` is called with a degenerate URL like `"http://"` (scheme with no host), it returns an empty string `""`. The caller in `tool_registry.rs:233-247` then checks `if !state.can_access_domain(host)` with `host = ""`. If the capability whitelist uses substring or prefix matching, an empty string might match everything or nothing depending on the implementation. The `can_access_domain` and `DomainPattern` implementations in `capabilities.rs` were not in the review scope, but this edge case should be verified.

**Fix:** Return `None` from the domain check when the extracted host is empty, effectively denying the request:

```rust
if name == HTTP_REQUEST_TOOL_NAME {
    if let Some(url_str) = args.get("url").and_then(|v| v.as_str()) {
        let host = extract_host_from_url(url_str);
        if host.is_empty() {
            return ToolResult::from_error(
                "CAPABILITY_DENIED",
                "Could not extract a valid host from the provided URL.",
                false,
            );
        }
        if !state.can_access_domain(host) {
            // ... existing check
        }
    }
}
```

## Info

### IN-01: Duplicated augmented-prompt construction logic between run_agent and resume_session

**File:** `crates/jadepaw-agent/src/lib.rs:106-119` and `lib.rs:307-321`
**Issue:** The skill-augmented system prompt construction (checking `has_active_skills`, building tool definitions, delegating to `build_skill_augmented_prompt` or `build_system_prompt_with_tools`) is copy-pasted verbatim between `run_agent()` and `resume_session()`. This is 14 identical lines. Any future change to prompt construction logic must be made in two places.

**Fix:** Extract a private helper function:

```rust
fn build_augmented_prompt(
    skill_manager: Option<&SkillManager>,
    tool_registry: &ToolRegistry,
    tenant_id: TenantId,
) -> String {
    let system_prompt = llm::REACT_SYSTEM_PROMPT;
    let tool_definitions = tool_registry.list_tools();
    if let Some(sm) = skill_manager {
        if sm.has_active_skills(tenant_id) {
            let (skill_block, _tool_names) = sm.merge_active(tenant_id);
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
    }
}
```

### IN-02: ToolId type is missing From<Uuid> implementation unlike other ID types

**File:** `crates/jadepaw-core/src/types.rs:88-119`
**Issue:** `SessionId`, `TenantId`, and `SkillId` all implement `From<Uuid>`, but `ToolId` does not. This is an inconsistency that forces callers needing a Uuid->ToolId conversion to use a different pattern than for the other ID types. Currently no reviewed code needs this conversion, but it is a trap for future developers.

**Fix:** Add the missing implementation:

```rust
impl From<Uuid> for ToolId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}
```

### IN-03: Test in tool_registry.rs for call_tool cannot exercise capability check without a real SessionHandle

**File:** `crates/jadepaw-agent/src/tool_registry.rs:412-421`
**Issue:** The test `test_call_tool_unknown_returns_error` at line 412 states: "Since we can't easily construct a real SessionHandle, we test the lookup path via get_by_name + manual error construction." This means the `call_tool()` capability check and dispatch paths are untested in the unit test suite. The comment acknowledges the limitation but doesn't link to a tracking issue or note when this gap will be closed.

**Fix:** Add a reference to a tracking issue or add an integration test that exercises the full `call_tool()` path with a real `SessionHandle`.

### IN-04: Unused import — `ChatCompletionRequestUserMessageContent` imported but only used in test

**File:** `crates/jadepaw-agent/src/window.rs:39`
**Issue:** `ChatCompletionRequestUserMessageContent` is imported at line 39 and used at line 285-294 (in the non-test `build_summary` function), so this is not dead code. However, `chat::` types are imported individually in `loop.rs` (lines 21-24) but using a different pattern than `llm.rs` (lines 18-25). Consider consolidating to a common `types` module for consistency. This is purely stylistic.

---

_Reviewed: 2026-06-06T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_