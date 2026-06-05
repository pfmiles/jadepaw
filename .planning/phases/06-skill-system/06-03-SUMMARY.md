---
phase: 06-skill-system
plan: 03
subsystem: api
tags: [sqlx, axum, sqlite, walkdir, skill-persistence]

# Dependency graph
requires:
  - phase: 06-01
    provides: "SkillId, SkillManifest, SkillValidationError types; SKILL.md parser; workspace deps"
  - phase: 06-02
    provides: "SkillRegistry, SkillManager, SkillInjector; AgentRequest.skills field"
provides:
  - "SkillRepository trait with dual-key (skill_id, tenant_id) isolation"
  - "SqliteSkillRepo backed by shared SqlitePool"
  - "skill_index SQLite table for skill metadata caching"
  - "SkillLoader walkdir scanner for SKILL.md discovery"
  - "SkillIndex parse+sync to SQLite cache"
  - "REST API: POST /skills/load, POST /skills/unload, GET /skills/list, GET /skills/inspect/{name}"
  - "Server startup sequence: DB pool, migrations, walkdir scan, index sync, axum serve"
affects: [07-chat-ui, 08-skill-management-ui]

# Tech tracking
tech-stack:
  added: [sqlx 0.9 (server), walkdir 2.5 (skill), anyhow 1.0 (server/skill), tempfile (skill dev)]
  patterns:
    - "Dual-key isolation: every DB query requires both skill_id and tenant_id"
    - "UUID BLOB binding: .bind(id.as_bytes().as_slice()) for SQLite storage"
    - "Filesystem as source of truth: SQLite index is cache, inspect reads from disk"
    - "walkdir scan via spawn_blocking: synchronous file I/O off tokio runtime"

key-files:
  created:
    - crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql
    - crates/jadepaw-db/src/skill_models.rs
    - crates/jadepaw-db/src/skill_repository.rs
    - crates/jadepaw-db/src/sqlite_skill_repo.rs
    - crates/jadepaw-skill/src/loader.rs
    - crates/jadepaw-skill/src/index.rs
    - crates/jadepaw-server/src/routes/skills.rs
    - crates/jadepaw-server/src/routes/mod.rs
  modified:
    - crates/jadepaw-db/src/lib.rs
    - crates/jadepaw-skill/src/lib.rs
    - crates/jadepaw-skill/Cargo.toml
    - crates/jadepaw-skill/src/manager.rs
    - crates/jadepaw-server/src/main.rs
    - crates/jadepaw-server/Cargo.toml

key-decisions:
  - "SqliteSkillRepo receives shared SqlitePool (no separate pool) — same pattern as SqliteSessionRepo"
  - "SkillManager::skills_root made pub for path construction in inspect_skill handler (alternative: getter method)"
  - "inspect_skill reads directly from filesystem (source of truth per D-09), not from SQLite cache"
  - "Startup scan executed via spawn_blocking to avoid blocking tokio runtime (Pitfall 4)"
  - "Invalid SKILL.md files logged as warnings and skipped during sync — one broken file does not block others"

patterns-established:
  - "Dual-key repository: every trait method takes both resource_id and tenant_id as mandatory params"
  - "UUID BLOB pattern: bind .as_bytes().as_slice(), extract via Uuid::from_slice(&raw)"
  - "Timestamp pattern: store as RFC3339 via chrono::to_rfc3339(), parse via parse_from_rfc3339()"
  - "Migration auto-discovery: sqlx::migrate!() picks up new SQL files in sorted order"

requirements-completed: [SKILL-01, SKILL-02]

# Metrics
duration: 16min
completed: 2026-06-05
---

# Phase 06 Plan 03: Skill Persistence and REST API Summary

**SQLite-backed skill metadata index, walkdir startup discovery, and four axum endpoints (load/unload/list/inspect) connecting the skill runtime to external consumers**

## Performance

- **Duration:** 16 min
- **Started:** 2026-06-05T16:56:33Z
- **Completed:** 2026-06-05T17:12:59Z
- **Tasks:** 3
- **Files modified:** 15

## Accomplishments
- SkillRepository trait with dual-key (skill_id, tenant_id) isolation following SessionRepository pattern
- SqliteSkillRepo implementation using shared SqlitePool with WAL mode, UUID BLOB binding, and raw sqlx::query()
- skill_index table migration with tenant+name and tenant+created_at composite indexes
- SkillLoader with walkdir-based SKILL.md file discovery supporting multi-tenant directory layout (global/ and <tenant_id>/)
- SkillIndex parse+sync layer that reads SKILL.md files, validates them, and bulk-upserts to SQLite
- Four REST API endpoints: load (POST), unload (POST), list (GET), inspect (GET /{name})
- Server startup sequence: SQLite pool creation, migration, walkdir scan via spawn_blocking, index sync, axum serve on 127.0.0.1:3000
- 67 tests passing in jadepaw-skill (includes 10 new loader/index tests)
- Full workspace compiles with no errors

## Task Commits

Each task was committed atomically:

1. **Task 1: SkillRepository trait, SqliteSkillRepo, and DB migration** - `414a294` (feat)
2. **Task 2: SkillLoader, SkillIndex, and startup scan integration** - `e5674ae` (feat)
3. **Task 3: REST API endpoints and server startup** - `61aab38` (feat)

## Files Created/Modified
- `crates/jadepaw-db/migrations/20260605000002_create_skill_index.sql` - SQLite DDL for skill metadata cache table
- `crates/jadepaw-db/src/skill_models.rs` - SkillIndexRecord and SkillIndexSummary data models
- `crates/jadepaw-db/src/skill_repository.rs` - SkillRepository trait with sync_index, list_by_tenant, get_by_name, delete
- `crates/jadepaw-db/src/sqlite_skill_repo.rs` - SQLite implementation of SkillRepository with dual-key isolation
- `crates/jadepaw-db/src/lib.rs` - Module declarations and re-exports for skill types
- `crates/jadepaw-skill/src/loader.rs` - SkillLoader walkdir scanner with tenant directory awareness
- `crates/jadepaw-skill/src/index.rs` - SkillIndex parse+sync to SQLite cache
- `crates/jadepaw-skill/src/lib.rs` - Activated loader and index modules with re-exports
- `crates/jadepaw-skill/Cargo.toml` - Added jadepaw-db, walkdir, anyhow, uuid, tempfile dependencies
- `crates/jadepaw-skill/src/manager.rs` - Made skills_root field public for API handler access
- `crates/jadepaw-server/src/routes/skills.rs` - Four axum route handlers for skill CRUD
- `crates/jadepaw-server/src/routes/mod.rs` - Route module declarations
- `crates/jadepaw-server/src/main.rs` - Full server startup: DB init, migration, scan, sync, axum serve
- `crates/jadepaw-server/Cargo.toml` - Added jadepaw-db, sqlx, serde, serde_json, chrono, anyhow, uuid, tokio deps

## Decisions Made
- SqliteSkillRepo receives shared SqlitePool (no separate pool creation) — same pattern as SqliteSessionRepo
- SkillManager::skills_root made pub for path construction in inspect_skill handler (minimal change, direct access)
- inspect_skill reads directly from filesystem (source of truth per D-09), not from SQLite cache
- Startup walkdir scan executed via tokio::task::spawn_blocking to avoid blocking the async runtime (RESEARCH.md Pitfall 4)
- Invalid SKILL.md files logged as warnings and skipped during sync — one broken file does not block others

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added missing anyhow and uuid dependencies to jadepaw-skill**
- **Found during:** Task 2 compilation
- **Issue:** index.rs uses anyhow::Result and uuid::Uuid but these were not in jadepaw-skill/Cargo.toml
- **Fix:** Added anyhow = "1.0" and uuid = { workspace = true } to jadepaw-skill dependencies
- **Files modified:** crates/jadepaw-skill/Cargo.toml
- **Committed in:** e5674ae (Task 2 commit)

**2. [Rule 2 - Missing Critical] Added tempfile dev-dependency for loader tests**
- **Found during:** Task 2 test compilation
- **Issue:** loader.rs tests use tempfile::tempdir() but tempfile was not in dev-dependencies
- **Fix:** Added [dev-dependencies] section with tempfile = "*"
- **Files modified:** crates/jadepaw-skill/Cargo.toml
- **Committed in:** e5674ae (Task 2 commit)

**3. [Rule 1 - Bug] Fixed inspect_skill return type from impl IntoResponse to Response**
- **Found during:** Task 3 compilation
- **Issue:** Type inference failure — different match arms returned different concrete IntoResponse types, causing "size for str cannot be known" errors
- **Fix:** Changed return type to axum::response::Response and added Response import
- **Files modified:** crates/jadepaw-server/src/routes/skills.rs
- **Committed in:** 61aab38 (Task 3 commit)

**4. [Rule 3 - Blocking] Made SkillManager::skills_root public for inspect handler access**
- **Found during:** Task 3 compilation
- **Issue:** inspect_skill handler needed to access skills_root for path construction, but the field was private
- **Fix:** Changed skills_root field visibility from private to pub in SkillManager struct
- **Files modified:** crates/jadepaw-skill/src/manager.rs
- **Committed in:** 61aab38 (Task 3 commit)

---

**Total deviations:** 4 auto-fixed (2 missing critical, 1 bug, 1 blocking)
**Impact on plan:** All auto-fixes necessary for compilation and test correctness. No scope creep.

## Issues Encountered
- **CWD drift (#3097):** The Write and Edit tools used absolute paths pointing to the main repo rather than the worktree, requiring manual file copies between repos. This did not affect correctness but added overhead.
- **Protected ref commit:** Task 1 commit accidentally landed on `main` (43ae914) before being cherry-picked to worktree branch (414a294). The `main` commit was not removed per the destructive git prohibition. The orchestrator will need to handle this.
- **Pre-existing test failure:** `jadepaw-agent` termination test `run_with_guard_maps_loop_error_to_wasm_trap` fails expecting WasmTrap but receiving InfrastructureError. This is unrelated to the skill system and predates this plan.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Skill persistence and API layer complete — Phase 6 vertical slice is now fully functional
- Skills can be created as SKILL.md files, discovered at startup, loaded/unloaded via API, and immediately influence agent behavior
- Ready for Phase 7 (Web Chat UI) and Phase 8 (Skill Management UI) to build UI on top of these endpoints
- Threat model mitigations T-06-10 through T-06-14 are implemented (path traversal prevention, dual-key isolation, spawn_blocking, JSON validation)
- T-06-13 (no authentication) is accepted risk for MVP per the threat model

---
*Phase: 06-skill-system*
*Completed: 2026-06-05*