# Phase 1: Project Foundation - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-05-28
**Phase:** 1-project-foundation
**Areas discussed:** Crate granularity & dependency graph, Feature flag strategy, CI platform & pipeline design, Dev tooling & conventions, Project initialization structure

---

## Crate Granularity & Dependency Graph

| Option | Description | Selected |
|--------|-------------|----------|
| 7 crates (as documented) | Exactly the 7 documented crates. core accumulates shared types/errors/config. Split later if needed. | ✓ |
| 8 crates (add jadepaw-macros) | Dedicated proc-macro crate. Needed by compiler mandate for derive macros. | |
| 8 crates (add jadepaw-common) | Shared types crate (SessionId, TenantId, ToolId). core becomes traits/re-exports. | |
| 9 crates (add both) | Maximum future-proofing but 9 crates for greenfield. | |

**User's choice:** 7 crates (as documented)
**Notes:** Splitting core or adding macros is a 2-file refactor later, not architectural rework. No premature decomposition.

---

## Feature Flag Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Hybrid: workspace + per-crate | Root Cargo.toml aggregate features + per-crate #[cfg] gates + compile_error! guards. | ✓ |
| Minimal: default + cluster only | 2 features, simplest surface, matches STACK.md patterns. | |
| Runtime dispatch via traits | DatabaseBackend/CacheBackend traits, features control which impls compile. | |
| Per-crate only | Each crate defines features independently. No workspace orchestration. | |

**User's choice:** Hybrid: workspace + per-crate
**Notes:** Sub-crates must be independently testable. LLM providers remain fully runtime via Box<dyn Config> — never feature flags.

---

## CI Platform & Pipeline Design

| Option | Description | Selected |
|--------|-------------|----------|
| GitHub Actions (consensus stack) | Swatinem/rust-cache, cargo-nextest, Linux stable+beta + macOS stable, cargo-deny blocking for bans/licenses, cargo-audit on schedule. | ✓ |
| GitHub Actions (extended) | Consensus stack + Windows matrix + code coverage via cargo-llvm-cov + Codecov. | |

**User's choice:** GitHub Actions (consensus stack)
**Notes:** Code coverage deferred to Phase 2. CI speed optimizations: CARGO_INCREMENTAL=0, CARGO_PROFILE_DEV_DEBUG=0.

---

## Dev Tooling & Conventions

### rustfmt
| Option | Description | Selected |
|--------|-------------|----------|
| Ecosystem standard config | style_edition=2024, group_imports=StdExternalCrate, imports_granularity=Crate, max_width=100. | ✓ |
| Defaults only | No rustfmt.toml — all defaults. | |

### Clippy
| Option | Description | Selected |
|--------|-------------|----------|
| Pedantic warn + allow-list | pedantic=warn with targeted allows: similar_names, module_name_repetitions, cast_precision_loss, unreadable_literal. | ✓ |
| Defaults only | Default lints only, zero config. | |

### Task Runner
| Option | Description | Selected |
|--------|-------------|----------|
| just (justfile) | 34k stars, language-agnostic, recipe parameters, .env loading, subdirectory invocation. | ✓ |
| cargo-make (Makefile.toml) | Rust-native TOML config, cargo subcommand, more powerful but over-engineered. | |
| Makefile | Universal availability but tab sensitivity, poor errors, no Windows support. | |

### Pre-commit Hooks
| Option | Description | Selected |
|--------|-------------|----------|
| Custom git hooks | Shell script in .githooks/: cargo fmt --check + cargo clippy. Zero external deps. | ✓ |
| pre-commit framework | Standardized framework, automatic venv management, but adds Python dependency. | |
| CI-only | Rely on CI to catch formatting/lint failures. | |

**User's choice:** Ecosystem standard rustfmt, pedantic clippy, just task runner, custom git hooks.
**Notes:** .editorconfig and .gitattributes included from Day 1. cargo-deny bans openssl in favor of rustls.

---

## Project Initialization Structure

### Directory Layout
| Option | Description | Selected |
|--------|-------------|----------|
| crates/ subdirectory | All crates under crates/. Clean separation from docs/, .planning/. | ✓ |
| Flat: crates at repo root | Each crate at repo root. Simpler nesting but clutters root. | |

### Smoke Test
| Option | Description | Selected |
|--------|-------------|----------|
| Include from Day 1 | Single integration test importing all 7 crates. Catches forgotten pub use. | ✓ |
| Defer | cargo build --workspace + per-crate tests catch 95% of link issues. | |

### Placeholder Modules
| Option | Description | Selected |
|--------|-------------|----------|
| Doc-commented stubs | //! module-level doc comments in each lib.rs. Architecture onboarding aid. | ✓ |
| Empty lib.rs only | No maintenance, but crates look identical. | |

### Static Files
| Option | Description | Selected |
|--------|-------------|----------|
| crates/server/static/ now | Create directory with .gitkeep. ServeDir::new("static") resolves naturally. | ✓ |
| Defer to Phase 7 | Let Phase 7 decide. | |

**User's choice:** crates/ subdirectory, include smoke test, doc-commented stubs, static/ now.
**Notes:** .planning/ kept visible in git. .gitignore: Rust standard template.

---

## Deferred Ideas

None — discussion stayed within phase scope.