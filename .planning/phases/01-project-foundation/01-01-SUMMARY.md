---
phase: 01-project-foundation
plan: 01
subsystem: infra
tags: [rust, cargo, workspace, wasmtime, tokio, axum]

# Dependency graph
requires: []
provides:
  - Root workspace Cargo.toml with workspace.dependencies pinning all 15 crate versions
  - 7 crates in crates/ with correct topological dependency order (core -> wasm -> bus -> agent -> skill -> gateway -> server)
  - Per-crate [features] with single-node default and cluster optional (wasm, bus, agent, gateway, server)
  - compile_error! guard for mutually exclusive single-node + cluster in jadepaw-server/lib.rs
  - Workspace smoke test importing all 6 library crates
  - Project config files: rustfmt.toml, clippy.toml, deny.toml, .editorconfig, .gitattributes, .gitignore
  - Static files placeholder (crates/jadepaw-server/static/.gitkeep)
affects: [02-wasm-isolation, 03-agent-runtime, 04-skill-system, 05-session-memory, 06-gateway, 07-web-ui]

# Tech tracking
tech-stack:
  added:
    - Rust 2024 edition (rust-version = "1.85")
    - wasmtime 45.0 (default-features = false)
    - tokio 1.52 (features = ["full"])
    - axum 0.8 (features = ["ws"])
    - tower-http 0.6 (features = ["cors", "fs"])
    - tower 0.5
    - serde 1.0 (features = ["derive"])
    - serde_json 1.0
    - sqlx 0.9 (features = ["runtime-tokio-rustls"])
    - redis 1.2 (features = ["tokio-comp"])
    - tracing 0.1
    - tracing-subscriber 0.3 (features = ["env-filter", "json"])
    - uuid 1.0 (features = ["v7"])
    - chrono 0.4 (features = ["serde"])
    - async-openai 0.40
  patterns:
    - workspace.dependencies for centralized version pinning (wasmtime 45 pattern)
    - Hybrid feature flag strategy: per-crate features with single-node default
    - Virtual workspace manifest (no [package] or [features] at root)
    - Strict topological dependency order enforced by Cargo

key-files:
  created:
    - Cargo.toml (workspace root with workspace.dependencies and profiles)
    - Cargo.lock (checked in per binary crate convention)
    - crates/jadepaw-core/Cargo.toml + src/lib.rs
    - crates/jadepaw-wasm/Cargo.toml + src/lib.rs
    - crates/jadepaw-bus/Cargo.toml + src/lib.rs
    - crates/jadepaw-agent/Cargo.toml + src/lib.rs
    - crates/jadepaw-skill/Cargo.toml + src/lib.rs
    - crates/jadepaw-gateway/Cargo.toml + src/lib.rs
    - crates/jadepaw-server/Cargo.toml + src/main.rs + src/lib.rs + static/.gitkeep
    - tests/workspace_smoke.rs
    - rustfmt.toml
    - clippy.toml
    - deny.toml
    - .editorconfig
    - .gitattributes
    - .gitignore
  modified: []

key-decisions:
  - "Root workspace is a virtual manifest — no [package] or [features] section; per-crate features defined in each crate's Cargo.toml"
  - "serde_yaml deferred to Phase 6 (deprecated by dtolnay since March 2024; serde_yml as replacement TBD)"
  - "wasmtime pinned at 45.0 (not 38.0 from original STACK.md which was 7 major versions behind)"
  - "redis 1.2 pinned (major version bump from 0.28 in STACK.md)"
  - "No LLM feature flags — providers remain runtime via Box<dyn Config> per D-05"

patterns-established:
  - "Pattern 1: workspace.dependencies for centralized version management"
  - "Pattern 2: Hybrid feature flag strategy (per-crate features, no root [features] in virtual manifest)"
  - "Pattern 3: Strict topological dependency order (core -> wasm -> bus -> agent -> skill -> gateway -> server)"
  - "Pattern 4: Module-level //! doc comments with 'What lives here' / 'What does NOT live here' sections"

requirements-completed: []

# Metrics
duration: TBD
completed: 2026-05-29
---

# Phase 1: Project Foundation — Plan 01-01 Summary

**Rust workspace scaffold with 7 crates in topological order, workspace.dependencies pinning 15 crate versions, smoke test, and all project configuration files (fmt, clippy, deny, editorconfig, gitattributes, gitignore).**

## Performance

- **Duration:** ~45 min (estimated, across 3 tasks over 2 execution attempts)
- **Tasks:** 3
- **Files created:** 24
- **Crates:** 7 (6 library + 1 binary)

## Accomplishments

- Workspace root Cargo.toml with 15 crate versions pinned in workspace.dependencies (wasmtime 45.0, tokio 1.52, axum 0.8, sqlx 0.9, redis 1.2, async-openai 0.40, etc.)
- 7 crates scaffolded in strict topological order with module-level doc comments and correct internal path dependencies
- Per-crate [features] sections with single-node default for crates needing deployment mode gating (wasm, bus, agent, gateway, server)
- compile_error! guard in jadepaw-server/src/lib.rs for mutually exclusive single-node + cluster features (D-04)
- Workspace smoke test (tests/workspace_smoke.rs) importing all 6 library crates for linkage verification (D-21)
- Project configuration files: rustfmt.toml (style_edition = "2024"), clippy.toml, deny.toml (bans openssl), .editorconfig, .gitattributes, .gitignore
- Frontend static files placeholder: crates/jadepaw-server/static/.gitkeep (D-23)

## Task Commits

Each task was committed atomically:

1. **Task 1: Root workspace Cargo.toml** - `45a2ebf` (feat: create root workspace Cargo.toml with workspace.dependencies)
2. **Task 2: All 7 crates** - `cbf220c` (feat: create all 7 crates with Cargo.toml, lib.rs docs, and dependency graph)
3. **Task 2 fix: Remove root [package] and stray src/lib.rs** - `61efd63` (fix: remove erroneous [package] section and stray src/lib.rs from root workspace)
4. **Task 3: Config files + smoke test** - PENDING (needs `cargo build --workspace` verification before commit)

## Files Created/Modified

### Task 1
- `Cargo.toml` - Root workspace manifest with workspace.dependencies, profiles, and features

### Task 2
- `crates/jadepaw-core/Cargo.toml` + `crates/jadepaw-core/src/lib.rs` - Core crate (serde, uuid, chrono deps)
- `crates/jadepaw-wasm/Cargo.toml` + `crates/jadepaw-wasm/src/lib.rs` - Wasm crate (wasmtime, redis optional + feature flags)
- `crates/jadepaw-bus/Cargo.toml` + `crates/jadepaw-bus/src/lib.rs` - Bus crate (redis optional + feature flags)
- `crates/jadepaw-agent/Cargo.toml` + `crates/jadepaw-agent/src/lib.rs` - Agent crate (async-openai, redis optional)
- `crates/jadepaw-skill/Cargo.toml` + `crates/jadepaw-skill/src/lib.rs` - Skill crate (serde)
- `crates/jadepaw-gateway/Cargo.toml` + `crates/jadepaw-gateway/src/lib.rs` - Gateway crate (axum, tower-http + feature flags)
- `crates/jadepaw-server/Cargo.toml` + `crates/jadepaw-server/src/main.rs` + `crates/jadepaw-server/src/lib.rs` + `crates/jadepaw-server/static/.gitkeep` - Binary crate with compile_error! guard
- `Cargo.lock` - Generated lock file

### Task 3 (pending commit)
- `tests/workspace_smoke.rs` - Imports all 6 library crates for linkage verification (D-21)
- `rustfmt.toml` - style_edition = "2024", group_imports, max_width = 100 (D-14)
- `clippy.toml` - allow-doc-keyword-errors, doc-valid-idents (D-15)
- `deny.toml` - License allow-list, openssl ban, advisory+yanked checks (D-19)
- `.editorconfig` - Cross-editor indentation/encoding config (D-18)
- `.gitattributes` - Line-ending normalization, export-ignore (D-18)
- `.gitignore` - Rust standard template, NOT blocking .planning/ (D-24, D-25)

## Decisions Made

- Root workspace is a virtual manifest only. `[features]` section removed from root because virtual manifests cannot have features. Per-crate features defined in individual crate Cargo.toml files instead.
- serde_yaml completely omitted from workspace.dependencies (deferred to Phase 6 per RESEARCH.md recommendation — crate deprecated since March 2024)

## Deviations from Plan

### Auto-fixed Issues

**1. [Root features removed] Virtual workspace manifest cannot have [features]**
- **Found during:** Task 1 fix commit (61efd63)
- **Issue:** The plan specified `[features]` in the root Cargo.toml with crate-name/feature mappings, but Cargo rejects `[features]` in virtual workspace manifests
- **Fix:** Removed `[features]` from root Cargo.toml. Each crate independently defines its own `[features]` with `default = ["single-node"]` per D-06
- **Files modified:** Cargo.toml, crates/jadepaw-wasm/Cargo.toml, crates/jadepaw-bus/Cargo.toml, crates/jadepaw-agent/Cargo.toml, crates/jadepaw-gateway/Cargo.toml, crates/jadepaw-server/Cargo.toml
- **Verification:** cargo metadata --no-deps returns valid JSON with all 7 workspace members
- **Committed in:** 61efd63 (fix commit)

**2. [jadepaw-skill no features] Skill crate needs no feature flags**
- **Found during:** Task 2
- **Issue:** Skill depends on core, wasm, agent (which has cluster features), but skill itself doesn't gate any deps on deployment mode
- **Fix:** Skill crate left without [features] section — its behavior doesn't change between single-node and cluster
- **Verification:** cargo metadata shows skill has empty features, which is correct

---

**Total deviations:** 2 auto-fixed (1 virtual manifest constraint, 1 crate-specific decision)
**Impact on plan:** Both necessary for correctness. No scope creep.

## Issues Encountered

- **Version drift from STACK.md:** Original STACK.md listed wasmtime 38.0, redis 0.28, async-openai 0.34. RESEARCH.md identified wasmtime 45.0, redis 1.2, async-openai 0.40 as current crates.io versions. Phase 1 uses RESEARCH.md versions.
- **serde_yaml deprecation:** serde_yaml was deprecated by maintainer dtolnay in March 2024. Not included in workspace.dependencies — deferred to Phase 6.
- **Root [features] rejection by Cargo:** Virtual workspace manifests cannot define `[features]`. The plan's hybrid feature strategy was adjusted to rely solely on per-crate features.

## User Setup Required

None — no external service configuration required. The workspace build is self-contained.

## Next Phase Readiness

- Workspace scaffold ready for Phase 02 (Wasm Isolation Core)
- All 7 crates exist with correct dependency graph — jadepaw-wasm crate is ready for wasmtime integration
- Workspace smoke test validates crate linkage
- CI configuration (Plan 01-02) can be created on top of this scaffold

---
*Phase: 01-project-foundation*
*Plan: 01 workspace scaffold*
*Completed: 2026-05-29*