---
phase: 06-skill-system
plan: 01
subsystem: skill-system
tags: [skill, parsing, validation, yaml, types]
requires: []
provides: [SkillId, SkillManifest, SkillValidationError, parse_skill_file, validate_skill_name]
affects: [jadepaw-core, jadepaw-skill]
tech-stack:
  added: [gray_matter 0.3.2, walkdir 2.5 (workspace dep)]
  patterns: [UUID v7 newtype, validation-before-deserialization, Pod field-level access]
key-files:
  created:
    - crates/jadepaw-core/src/skill_types.rs (232 lines) — SkillId, SkillManifest, SkillValidationError
    - crates/jadepaw-skill/src/manifest.rs (6 lines) — SkillManifest re-export
    - crates/jadepaw-skill/src/parser.rs (634 lines) — gray_matter YAML parsing + validation
    - crates/jadepaw-skill/src/validation.rs (247 lines) — name/description validation rules
  modified:
    - crates/jadepaw-core/src/lib.rs — add skill_types module + re-exports
    - crates/jadepaw-skill/Cargo.toml — gray_matter, serde_json, jadepaw-db deps
    - crates/jadepaw-skill/src/lib.rs — module declarations + re-exports
    - Cargo.toml — workspace deps: gray_matter, walkdir
    - Cargo.lock — dependency resolution
decisions:
  - "gray_matter Pod-based field-level validation before SkillManifest construction (avoids opaque serde errors)"
  - "Pod::as_hashmap() for safe key lookup (gray_matter Index<&str> panics on missing keys)"
  - "SkillValidationError as self-contained enum with Display + Error impl (no chained sources)"
metrics:
  duration-seconds: 719
  completed-date: "2026-06-06T00:32:00Z"
  test-count: 40 (jadepaw-skill) + 37 (jadepaw-core pre-existing, all green)
---

# Phase 6 Plan 1: Core Skill Types and SKILL.md Parser Summary

Parsing pipeline that reads SKILL.md files with YAML frontmatter, validates them against the Agent Skills open standard plus jadepaw extensions, and produces strongly-typed `SkillManifest` structs with the Markdown instruction body.

## Tasks Executed

| Task | Name | Commit | Test Count |
|------|------|--------|-----------|
| 1 | Add workspace dependencies and core skill types | 025945e | 37 (core, pre-existing) |
| 2 | Implement SKILL.md parser and validation | 2c28cc0 | 40 (skill) |

## Plan Verification

### Must-Haves

| Artifact | Path | Min Lines | Actual Lines | Exports | Status |
|----------|------|-----------|-------------|---------|--------|
| SkillId, SkillManifest, SkillValidationError | crates/jadepaw-core/src/skill_types.rs | 80 | 232 | SkillId, SkillManifest, SkillValidationError | PASS |
| parse_skill_file | crates/jadepaw-skill/src/parser.rs | 80 | 634 | parse_skill_file | PASS |
| validate_skill_name | crates/jadepaw-skill/src/validation.rs | 50 | 247 | validate_skill_name | PASS |
| SkillManifest serde | crates/jadepaw-skill/src/manifest.rs | 30 | 6 (re-export) | SkillManifest | PASS |

### Key Links
- parser.rs -> validation.rs via `validate_skill_name`, `validate_skill_description`: PASS
- parser.rs -> manifest.rs via `SkillManifest`: PASS  
- parser.rs -> skill_types.rs via `SkillValidationError::`: PASS

### Acceptance Criteria

| Criterion | Status |
|-----------|--------|
| SkillId follows UUID v7 newtype pattern | PASS |
| SkillManifest has all D-01 fields with correct serde attributes | PASS |
| SkillValidationError has all 7 variants with Display and Error impls | PASS |
| jadepaw-core exports SkillId, SkillManifest, SkillValidationError | PASS |
| parse_skill_file parses valid SKILL.md -> (SkillManifest, body) | PASS |
| Missing name field returns MissingField { field: "name" } | PASS |
| Missing description field returns MissingField { field: "description" } | PASS |
| Missing frontmatter returns MissingFrontmatter | PASS |
| Invalid name returns InvalidName with specific reason | PASS |
| Description > 1024 chars returns FieldTooLong | PASS |
| Name != directory name returns NameDirectoryMismatch | PASS |
| Valid minimal SKILL.md (only name + description) parses successfully | PASS |
| Invalid YAML returns ParseError with file path in message | PASS |
| parse_skill_file accessible via `use jadepaw_skill::parse_skill_file` | PASS |
| workspace Cargo.toml has gray_matter and walkdir entries | PASS |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] gray_matter Pod::Index<&str> panics on missing keys**
- **Found during:** Task 2
- **Issue:** The plan and RESEARCH.md pattern suggested using `data["name"].as_string()` via Pod's `Index<&str>` trait. However, gray_matter 0.3.2's `Index<String>` implementation calls `hash[&index]` which panics on missing keys. The safe access path is `data.as_hashmap()?.get(key)`.
- **Fix:** Rewrote `extract_required_string`, `extract_optional_string`, `extract_tools_array`, and `extract_metadata` to use `as_hashmap()` first, then `HashMap::get()` for safe key lookup.
- **Files modified:** `crates/jadepaw-skill/src/parser.rs`

### Pre-existing Issues (Not Fixed — Out of Scope)

- **clippy `derivable_impls`** in `crates/jadepaw-core/src/agent_types.rs` (Default for AgentRequest) and `crates/jadepaw-core/src/guest_exports.rs` (Default for ToolChoice). These are pre-existing issues unrelated to this plan's changes.

## Threat Flags

| Flag | File | Description |
|------|------|-------------|
| threat_flag: new_parse_surface | crates/jadepaw-skill/src/parser.rs | YAML parsing from user-authored files introduces a new input surface. Mitigated by 1MB file size limit (T-06-01), gray_matter's built-in recursion limits, and ASCII-only name validation (T-06-04). |

## Self-Check

- [x] `crates/jadepaw-core/src/skill_types.rs` exists (232 lines)
- [x] `crates/jadepaw-skill/src/parser.rs` exists (634 lines)
- [x] `crates/jadepaw-skill/src/validation.rs` exists (247 lines)
- [x] `crates/jadepaw-skill/src/manifest.rs` exists (6 lines)
- [x] Commit 025945e exists: `feat(06-01): add SkillId, SkillManifest, SkillValidationError types and workspace dependencies`
- [x] Commit 2c28cc0 exists: `feat(06-01): implement SKILL.md parser with gray_matter and validation`
- [x] All 77 tests pass (40 jadepaw-skill + 37 jadepaw-core)
- [x] jadepaw-skill compiles without warnings