---
phase: 01-project-foundation
reviewed: 2026-05-29T03:00:00Z
depth: standard
files_reviewed: 28
files_reviewed_list:
  - Cargo.toml
  - crates/jadepaw-agent/Cargo.toml
  - crates/jadepaw-agent/src/lib.rs
  - crates/jadepaw-bus/Cargo.toml
  - crates/jadepaw-bus/src/lib.rs
  - crates/jadepaw-core/Cargo.toml
  - crates/jadepaw-core/src/lib.rs
  - crates/jadepaw-gateway/Cargo.toml
  - crates/jadepaw-gateway/src/lib.rs
  - crates/jadepaw-server/Cargo.toml
  - crates/jadepaw-server/src/main.rs
  - crates/jadepaw-server/src/lib.rs
  - crates/jadepaw-server/tests/workspace_smoke.rs
  - crates/jadepaw-skill/Cargo.toml
  - crates/jadepaw-skill/src/lib.rs
  - crates/jadepaw-wasm/Cargo.toml
  - crates/jadepaw-wasm/src/lib.rs
  - rustfmt.toml
  - clippy.toml
  - deny.toml
  - .editorconfig
  - .gitattributes
  - .gitignore
  - .github/workflows/ci.yml
  - .github/workflows/security-audit.yml
  - justfile
  - .githooks/pre-commit
  - .config/nextest.toml
findings:
  critical: 2
  warning: 6
  info: 8
  total: 16
status: issues_found
---

# Phase 01: Code Review Report

**Reviewed:** 2026-05-29T03:00:00Z
**Depth:** standard
**Files Reviewed:** 28
**Status:** issues_found

## Summary

Phase 01 established the Rust workspace scaffold with 7 crates, CI pipeline (GitHub Actions), and project configuration. The skeleton builds and passes CI green. However, the review found 2 critical issues, 6 warnings, and 8 informational items.

The most serious findings are: (1) the `jadepaw-skill` crate declares a dependency on `jadepaw-agent`, which contradicts the documented topological order in SKELETON.md (`core -> wasm -> bus -> agent -> skill -> gateway -> server`) and creates a DAG edge that does not match the architecture claim, and (2) the pre-commit hook runs clippy on `--all-targets` without `--workspace`, meaning it only lints the crate corresponding to the current working directory instead of the whole workspace.

## Critical Issues

### CR-01: Dependency DAG contradicts documented crate topology

**File:** `crates/jadepaw-skill/Cargo.toml:10`
**Issue:** The SKELETON.md (line "7 crates in strict topological order: core -> wasm -> bus -> agent -> skill -> gateway -> server") and the `jadepaw-agent/src/lib.rs` doc comment ("Skill format or compilation (see jadepaw-skill)") both describe the DAG as `agent -> skill`. However, the actual `jadepaw-skill/Cargo.toml` has `jadepaw-agent = { path = "../jadepaw-agent" }`, which means the real dependency is `skill -> agent`, reversing the claimed direction.

This is a correctness issue for two reasons:
1. Compile ordering: the claimed DAG says agent compiles before skill, but the actual Cargo dependency says skill depends on agent (agent must exist first), which is the opposite compile order.
2. Architecture integrity: if skill truly depends on agent types, then agent cannot depend on skill types without creating a potential circular dependency when agent types evolve that reference skill types. The intended architecture needs to be settled now, not deferred -- the circular dependency risk is structural.

**Fix:** Decide the intended dependency direction:
- If `skill -> agent` is correct: update both SKELETON.md and `jadepaw-agent/src/lib.rs` to say `core -> wasm -> bus -> skill -> agent -> gateway -> server`, and update `jadepaw-agent/src/lib.rs` "What does NOT live here" to remove the reference to jadepaw-skill or state that jadepaw-skill consumes agent interfaces.
- If `agent -> skill` is correct: remove `jadepaw-agent` from `jadepaw-skill/Cargo.toml` dependencies and add `jadepaw-skill` to `jadepaw-agent/Cargo.toml` dependencies.
- Most critically: document that the architecturally correct ordering prevents `jadepaw-agent` and `jadepaw-skill` from depending on each other in both directions (which would cause a Cargo circular dependency error the moment either crate references types from the other).

### CR-02: Pre-commit hook clippy scope is narrower than CI/justfile

**File:** `.githooks/pre-commit:17`
**Issue:** The pre-commit hook runs `cargo clippy --all-targets -- -D warnings` (line 17), while CI and justfile both run `cargo clippy --workspace --all-targets --all-features -- -D warnings`. The pre-commit hook is missing both `--workspace` and `--all-features`.

Without `--workspace`, the hook lints only the crate in the current working directory (not the full workspace). Without `--all-features`, feature-gated code is not checked. This means a developer can commit code that would fail CI linting, defeating the purpose of the pre-commit gate.

**Fix:**
```shell
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Warnings

### WR-01: clippy.toml documents pedantic allow flags that CI does not apply

**File:** `clippy.toml:3-9`, `.github/workflows/ci.yml:31`
**Issue:** The `clippy.toml` header states that pedantic lint level configuration is "set via CLI flags in CI" and shows the full command `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -A clippy::similar_names ...`. However, the actual CI command on line 31 is `cargo clippy --workspace --all-targets --all-features -- -D warnings` -- none of the pedantic/allowed flags are included.

Currently this is latent because the crate bodies are empty. Once code is added, the CI lint output will differ from what the `clippy.toml` documents, causing confusion about whether pedantic lints are intended to be enforced.

**Fix:** Either add the pedantic flags to the CI command as documented in `clippy.toml`, or update the `clippy.toml` comment to reflect the actual CI configuration. If pedantic is delayed to a later phase, note that in both files.

### WR-02: .gitignore excludes .claude/ but CLAUDE.md is checked into the repo

**File:** `.gitignore:25`
**Issue:** The `.gitignore` excludes `.claude/` entirely, but CLAUDE.md (at repo root) provides project instructions that contributors may want version-controlled. The comment says "Worktree isolation directories (generated by Claude Code GSD workflow)" but GSD may generate files inside `.claude/` that overlap with intentionally committed project config.

If CLAUDE.md is intended to be committed (it is currently tracked), the blanket exclusion `.claude/` should not apply to the root-level CLAUDE.md. Currently CLAUDE.md is tracked because it was committed before the gitignore rule, but new contributors adding files to `.claude/` may accidentally lose version control on project-level configuration.

**Fix:** Replace `.claude/` with more targeted exclusions:
```gitignore
# Worktree isolation directories (generated by Claude Code GSD workflow)
.claude/commands/
.claude/agents/
.claude/planning/
.claude/sessions/
.claude/stats/
```

### WR-03: justfile `check-all` omits `audit` recipe

**File:** `justfile:44`
**Issue:** The `check-all` recipe runs `fmt-check lint deny` (format, clippy, cargo-deny) but does not include `audit` (cargo-audit vulnerability scan). This is an inconsistency because `just audit` exists but is not part of the "check all" workflow. The SKELETON mentions `cargo-audit` as part of security infrastructure, and the security-audit CI workflow runs it. Developers running `just check-all` may assume all static checks pass, but vulnerabilities are not being scanned locally.

**Fix:**
```justfile
check-all: fmt-check lint deny audit
    @echo "==> All checks passed."
```

### WR-04: justfile `lint` recipe does not include pedantic lints even though clippy.toml documents them

**File:** `justfile:25`
**Issue:** Same as WR-01, but affecting the local development workflow. `just lint` runs `cargo clippy --workspace --all-targets --all-features -- -D warnings` without the pedantic `-W` and allow `-A` flags that `clippy.toml` claims are part of the configuration.

**Fix:** Align `just lint` with whatever decision is made for WR-01. Both CI and justfile should match.

### WR-05: jadepaw-gateway and jadepaw-server `cluster` feature does not enable `redis`

**File:** `crates/jadepaw-gateway/Cargo.toml:18`, `crates/jadepaw-server/Cargo.toml:20`
**Issue:** In `jadepaw-agent`, `jadepaw-bus`, and `jadepaw-wasm`, the `cluster` feature resolves to `["redis"]` (i.e., `cluster = ["redis"]`). But in `jadepaw-gateway` and `jadepaw-server`, `cluster = []` (empty, no sub-feature). This means enabling `cluster` on gateway or server does not pull in the `redis` dependency, even though cluster mode likely needs Redis (the SKELETON describes Redis for "session state cache, distributed locks, pub/sub"). The gateway (session registry, SSE streaming) and server (process lifecycle) both need Redis in cluster mode.

This is inconsistent and could cause silent failures where `cluster` mode runs without Redis, leading to in-memory state that does not actually work across nodes.

**Fix:** Either:
- Add `redis = ["dep:redis"]` and set `cluster = ["redis"]` in both gateway and server (consistent with other crates).
- Or document explicitly why gateway/server do not need Redis in cluster mode.

### WR-06: Axum feature flag missing in root Cargo.toml workspace dependencies

**File:** `Cargo.toml:20`
**Issue:** The axum workspace dependency specifies `axum = { version = "0.8", features = ["ws"] }` -- only WebSocket support. The SKELETON and `jadepaw-gateway/src/lib.rs` mention SSE (Server-Sent Events) for LLM token streaming. However, the `ws` feature enables `axum::extract::ws` (WebSocket) but SSE typically requires the `tokio` feature for `Sse::new()`. In axum 0.8, SSE support doesn't have a separate feature flag -- it uses `tokio` via `axum::response::Sse`. So this may not be a hard bug at the Cargo.toml level, but SSE depends on `tokio::sync::mpsc` channels being available, which they are through the `tokio = { features = ["full"] }` dependency.

Despite this specific case not being broken, the design choice to specify `features = ["ws"]` on axum only when SSE and other response helpers are also needed is worth noting -- as axum features expand, the workspace-level feature set should match what the gateway actually uses.

**Fix:** No immediate change needed. Monitor as the gateway crate is implemented. Consider `axum = { version = "0.8" }` (default features) if most features are used, to avoid future feature-gating surprises.

## Info

### IN-01: rustfmt unstable option comment may drift out of date

**File:** `rustfmt.toml:5-7`
**Issue:** The comment explains that `group_imports` and `imports_granularity` were removed because they are nightly-only. When these stabilize, the comment should be revisited. As a note-only item, this is not a bug, but stale config comments can mislead future contributors.

**Fix:** Add a periodic-review note or link to the tracking issue.

### IN-02: clippy.toml includes deprecated/transitioning lint groups

**File:** `clippy.toml:3-9`
**Issue:** The `clippy::pedantic` lint group is documented but not enabled. If the team intends to adopt pedantic lints, the `doc-valid-idents` list will be tested against pedantic doc lint rules. If not, the pedantic documentation is misleading.

**Fix:** Resolve in conjunction with WR-01.

### IN-03: Root Cargo.toml YAML support comment references Phase 6 with serde_yaml deprecation note

**File:** `Cargo.toml:27`
**Issue:** The comment says "YAML support deferred to Phase 6 (serde_yaml is deprecated since March 2024)". This is informative but should also mention the replacement to avoid contributors having to research: `serde_yml` (or whatever alternative is chosen).

**Fix:**
```toml
# YAML support deferred to Phase 6 (serde_yaml is deprecated since March 2024;
# plan to use serde_yml or serde_yaml replacement)
```

### IN-04: `jadepaw-server/src/main.rs` prints startup message but does not initialize tracing

**File:** `crates/jadepaw-server/src/main.rs:1-3`
**Issue:** The `main.rs` uses `println!` for a startup message. The crate depends on `tracing-subscriber` (with `env-filter` and `json` features), but `main.rs` does not initialize a tracing subscriber. This is acceptable for Phase 1 scaffold but means the tracing dependency is dead code until Phase 7.

**Fix:** Deferred to Phase 7 (web server). Not a bug for Phase 1 scope.

### IN-05: `workspace_smoke.rs` uses `#[allow(unused_imports)]` which would mask a real import failure

**File:** `crates/jadepaw-server/tests/workspace_smoke.rs:14-26`
**Issue:** Each `use jadepaw_* as *;` statement has `#[allow(unused_imports)]` before it. While the test comment explains this is needed because the crates are empty, allowing unused imports globally means that if a crate's `lib.rs` becomes empty or inaccessible due to a missing `pub` re-export, the test would still pass (silently). A more robust approach would be to reference an actual public item from each crate (e.g., a `pub mod` or re-export) once those exist.

**Fix:** Once crates have public API surface, add a minimal assertion per crate (e.g., `let _ = jadepaw_core::some_type;`) and remove the `#[allow(unused_imports)]` attributes. Consider adding a `// TODO(tests): replace with actual type assertions once crates have public API` comment.

### IN-06: cargo-deny `exceptions = []` and `ignore = []` are explicitly empty

**File:** `deny.toml:16`, `deny.toml:37`
**Issue:** Empty `exceptions` and `ignore` lists are explicit but add no value beyond default behavior. This is a minor style issue -- empty lists that mirror defaults can create confusion about whether they were intentionally empty vs forgotten.

**Fix:** Either remove the empty keys or add a comment: `# No exceptions at this time`.

### IN-07: security-audit.yml schedules weekly but could miss critical windows

**File:** `.github/workflows/security-audit.yml:4`
**Issue:** The `cron: "0 8 * * 1"` (Monday 8am) schedule means a vulnerability disclosed on Monday afternoon would not be detected until the following Monday morning. The workflow also runs on `Cargo.lock` changes, which provides faster feedback for dependency updates, but a vulnerability in a transitive dependency that was already in the tree could go undetected for up to 7 days.

**Fix:** Consider daily scheduling (`cron: "0 8 * * *"`) or more frequent cadence for the automatic scan. This is a policy decision, not a bug.

### IN-08: .gitattributes excludes only `target/` from archives

**File:** `.gitattributes:13`
**Issue:** Only `target/` is marked `export-ignore`. Other generated directories like `.planning/` or potential build artifacts are not excluded from `git archive` output. This is minor since `git archive` is rarely used for this project.

**Fix:** Low priority. Consider adding other directories if they bloat archives.

---

_Reviewed: 2026-05-29T03:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_