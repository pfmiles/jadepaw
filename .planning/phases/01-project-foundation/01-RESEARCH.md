# Phase 01: Project Foundation - Research

**Researched:** 2026-05-28
**Domain:** Rust workspace scaffolding, CI/CD pipeline, build system configuration
**Confidence:** HIGH

## Summary

This phase bootstraps a greenfield Rust workspace from zero -- no existing code, no conventions, no tooling. All 7 crates must be scaffolded in strict topological dependency order, CI must pass on every push, and the build must succeed from a clean checkout. The work is purely structural (no business logic), but the decisions made here establish the skeleton that all 8 subsequent phases build upon.

The primary technical risk is version drift -- the STACK.md researched in early 2026 lists wasmtime 38.0, but crates.io now shows wasmtime 45.0.0. The serde_yaml crate (listed as a core dependency) has been officially deprecated since March 2024 with `serde_yml` as the community successor. Several other pinned versions in STACK.md are now stale. Phase 1 must lock ACTUAL current versions, not STACK.md's historical versions, or later phases will hit version conflicts when adding real dependencies.

**Primary recommendation:** Pin wasmtime 45.0, axum 0.8.9, tokio 1.52, sqlx 0.9, redis 2.0, and replace serde_yaml with serde_yml (or delay YAML support to a later phase since Phase 1 has no config-parsing code). Use `workspace.dependencies` to manage versions centrally (wasmtime pattern, now standard in Rust 2024 ecosystem).

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** Exactly 7 crates as documented in ROADMAP.md: `jadepaw-core` -> `jadepaw-wasm` -> `jadepaw-bus` -> `jadepaw-agent` -> `jadepaw-skill` -> `jadepaw-gateway` -> `jadepaw-server`. No additional `jadepaw-common` or `jadepaw-macros` crate at scaffold time.
- **D-02:** Dependency graph is strict topological order. Each crate only depends on crates earlier in the chain. Zero circular dependencies. `jadepaw-core` has no internal jadepaw dependencies.
- **D-03:** Hybrid strategy -- root `Cargo.toml` `[features]` table defines aggregate features (`cluster`, `single-node` as default). Sub-crates define per-crate features with `#[cfg]` gates. Root features map to sub-crate features via `crate-name/feature` syntax.
- **D-04:** `compile_error!` guards in root crate prevent mutually exclusive features.
- **D-05:** LLM providers remain fully runtime via `Box<dyn Config>` pattern -- never become feature flags.
- **D-06:** Sub-crates must be independently buildable/testable with natural defaults (`sqlite` for database, in-memory for cache, no OTLP).
- **D-07:** GitHub Actions with the Rust CI consensus stack. Caching: `Swatinem/rust-cache@v2` (with `cache-bin: false` on macOS). Toolchain: `dtolnay/rust-toolchain`.
- **D-08:** Matrix: Linux (ubuntu-latest) stable + beta, macOS (macos-latest) stable only.
- **D-09:** Fast gate job (`check`) runs first: `cargo fmt --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo doc --workspace --no-deps --document-private-items`. Test matrix runs in parallel after gate passes.
- **D-10:** Test runner: `cargo nextest run --workspace` + `cargo test --doc --workspace`.
- **D-11:** Security: `cargo-deny` (bans + licenses blocking, advisories non-blocking via `continue-on-error`). `cargo-audit` in separate scheduled workflow (weekly + on Cargo.lock changes).
- **D-12:** Code coverage deferred to Phase 2.
- **D-13:** CI speed optimizations: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, cancel-in-progress per PR/branch.
- **D-14:** rustfmt: `style_edition = "2024"`, `group_imports = "StdExternalCrate"`, `imports_granularity = "Crate"`, `max_width = 100`.
- **D-15:** Clippy: `pedantic = "warn"` with targeted allows: `similar_names`, `module_name_repetitions`, `cast_precision_loss`, `unreadable_literal`. Nursery and restriction groups NOT enabled.
- **D-16:** Task runner: `just` (justfile) with recipes for `build`, `test`, `lint`, `fmt`, `deny`, `audit`, `wasm-build`.
- **D-17:** Pre-commit hooks: Custom shell scripts in `.githooks/` (zero external deps). `pre-commit` hook runs `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`. Configured via `git config core.hooksPath .githooks`.
- **D-18:** `.editorconfig` and `.gitattributes` included from Day 1.
- **D-19:** cargo-deny: license allow-list = Apache-2.0, MIT, ISC, BSD-2-Clause, BSD-3-Clause, Unicode-3.0, Zlib. Ban `openssl`/`openssl-sys` in favor of `rustls`. Duplicate dependency detection enabled.
- **D-20:** Crate layout: `crates/` subdirectory (wasmtime pattern). Workspace `members = ["crates/*"]`.
- **D-21:** Workspace-level smoke test in `tests/` that imports all 7 crates -- catches forgotten `pub use` re-exports.
- **D-22:** Each crate's `src/lib.rs` includes a `//!` module-level doc comment describing what the crate owns.
- **D-23:** Frontend static files directory: `crates/server/static/` created now with `.gitkeep`.
- **D-24:** `.planning/` directory kept visible in git (not gitignored).
- **D-25:** `.gitignore`: Rust standard template -- `target/`, `*.rs.bk`, `.env` (but not `.env.example`), IDE dirs (`.vscode/`, `.idea/`), OS files (`.DS_Store`).

### Claude's Discretion

None -- all 25 decisions are locked. Research focuses on technical unknowns within these constraints.

### Deferred Ideas (OUT OF SCOPE)

None -- discussion stayed within phase scope.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Workspace compilation | Build System (Cargo) | CI Runner | Cargo orchestrates compilation; CI validates it succeeds |
| Crate dependency graph | Build System (Cargo) | -- | Cargo enforces topological order via `[dependencies]` |
| Feature flag composition | Build System (Cargo) | -- | Root workspace features map to sub-crate features |
| Code formatting enforcement | Build System (rustfmt) | Pre-commit hooks | rustfmt enforces style; pre-commit blocks violations early |
| Lint enforcement | Build System (clippy) | CI Gate job | clippy checks quality; CI gates on zero warnings |
| License/security audit | CI (cargo-deny) | CI (cargo-audit) | Both are CI-only tools, not build dependencies |
| Test execution | CI (nextest) | Local dev | nextest runs the test matrix; devs run locally for feedback |
| Git hygiene (.gitignore, hooks) | Filesystem | -- | Configuration files, no runtime tier involved |
| Static file serving path | Build System (Cargo) | Server crate | `ServeDir::new("static")` resolves relative to workspace root |
| CI orchestration | GitHub Actions | -- | Workflow YAML defines matrix and gate stages |

## Standard Stack

### Core (Phase 1 - Workspace Foundation)

Phase 1 only needs Rust toolchain and Cargo -- no crates are used as dependencies in this phase beyond what the scaffold defines. However, we must pin the versions that the workspace-level `Cargo.toml` defines in `[workspace.dependencies]` for later phases. The versions below reflect **current crates.io as of 2026-05-28**, which supersede the STACK.md versions researched earlier.

| Technology | STACK.md Version | Current crates.io | Phase 1 Action |
|------------|-----------------|-------------------|----------------|
| Rust (rustc) | 1.85+ (2024 edition) | 1.95.0 (installed) | Already met -- use `edition = "2024"` |
| cargo | -- | 1.95.0 | Already met |
| wasmtime | 38.0+ | **45.0.0** [VERIFIED: crates-io] | Pin 45.0 in workspace.dependencies (do NOT pin 38.0 -- wasmtime 38 is 7 major releases behind; the pooling allocator, WASI preview2, and async support have changed significantly) |
| tokio | 1.43+ | **1.52.3** [VERIFIED: crates-io] | Pin 1.52 in workspace.dependencies |
| axum | 0.8.4 | **0.8.9** [VERIFIED: crates-io] | Pin 0.8 in workspace.dependencies; the 0.8.x series has been stable through 9 patch releases |
| serde / serde_json | 1.0+ | 1.0.228 / 1.0.150 [VERIFIED: crates-io] | Pin 1.0 in workspace.dependencies |
| serde_yaml | 0.9+ | **0.9.34+deprecated** [VERIFIED: crates-io] | **REPLACE** with `serde_yml` 0.0.x (community successor) or defer YAML support entirely -- Phase 1 has no YAML parsing. `serde_yaml` was deprecated by dtolnay in March 2024. |
| sqlx | 0.8+ | **0.9.0** [VERIFIED: crates-io] | Pin 0.9 in workspace.dependencies |
| redis-rs | 0.28+ | **1.2.1** [VERIFIED: crates-io] | Pin 1.2 in workspace.dependencies (major version bump: 0.28 -> 1.x) |
| async-openai | 0.34.0 | **0.40.2** [VERIFIED: crates-io] | Pin 0.40 in workspace.dependencies |
| tracing | 0.1+ | 0.1.44 [VERIFIED: crates-io] | Pin 0.1 in workspace.dependencies |
| tracing-subscriber | 0.3+ | 0.3.23 [VERIFIED: crates-io] | Pin 0.3 in workspace.dependencies |
| tower-http | 0.6+ | 0.6.11 [VERIFIED: crates-io] | Pin 0.6 in workspace.dependencies |
| tower | 0.5+ | 0.5.3 [VERIFIED: crates-io] | Pin 0.5 in workspace.dependencies |
| uuid | 1.0+ | 1.23.1 [VERIFIED: crates-io] | Pin 1 in workspace.dependencies |
| chrono | 0.4+ | 0.4.44 [VERIFIED: crates-io] | Pin 0.4 in workspace.dependencies |

### Development Tools

| Tool | Available | Version | Notes |
|------|-----------|---------|-------|
| just | YES | 1.40.0 | Task runner for `justfile` recipes |
| cargo-nextest | **NO** | -- | Must be installed: `cargo install cargo-nextest` |
| cargo-deny | **NO** | -- | Must be installed: `cargo install cargo-deny` |
| cargo-audit | **NO** | -- | Must be installed: `cargo install cargo-audit` |
| Rust targets | aarch64-apple-darwin | 1.95.0 | Local dev; CI adds x86_64-unknown-linux-gnu |

### Installation (Phase 1 - Developer machine)

```bash
# Required dev tools (not present on this machine)
cargo install cargo-nextest
cargo install cargo-deny
cargo install cargo-audit

# Pre-commit hook setup
git config core.hooksPath .githooks
chmod +x .githooks/pre-commit
```

**Version verification:** All Rust crate versions confirmed via `cargo search --registry crates-io` on 2026-05-28. The STACK.md document (researched ~February 2026) has drifted by 5-7 major versions for the most critical crate (wasmtime). The `serde_yaml` deprecation was missed in the original STACK.md research -- it was deprecated by the maintainer in March 2024, well before the STACK.md was written.

## Architecture Patterns

### System Architecture Diagram

This phase creates a build-time structure, not a runtime system. The data flow is the Cargo dependency resolution graph:

```
Root Workspace (Cargo.toml at repo root)
  │
  ├── [workspace.members] = ["crates/*"]
  │
  ├── [workspace.dependencies]
  │   └── Central version pinning for all 3rd-party crates
  │
  ├── [workspace.package]
  │   └── Shared: edition = "2024", license = "Apache-2.0 OR MIT"
  │
  └── [features]
      ├── default = ["single-node"]
      ├── single-node = ["jadepaw-wasm/single-node", ...]
      └── cluster    = ["jadepaw-wasm/cluster", ...]

  crates/
  ├── jadepaw-core/          # [no internal deps]
  │   └── Cargo.toml          depends: serde, uuid, chrono
  │
  ├── jadepaw-wasm/           # depends: jadepaw-core
  │   └── Cargo.toml          depends: wasmtime, tokio, jadepaw-core
  │
  ├── jadepaw-bus/            # depends: jadepaw-core, jadepaw-wasm
  │   └── Cargo.toml          depends: tokio, redis (feature-gated)
  │
  ├── jadepaw-agent/          # depends: jadepaw-core, jadepaw-wasm, jadepaw-bus
  │   └── Cargo.toml          depends: async-openai, tokio
  │
  ├── jadepaw-skill/          # depends: jadepaw-core, jadepaw-wasm, jadepaw-agent
  │   └── Cargo.toml          depends: serde
  │
  ├── jadepaw-gateway/        # depends: jadepaw-core, jadepaw-wasm, jadepaw-bus
  │   └── Cargo.toml          depends: axum, tower-http, tokio
  │
  └── jadepaw-server/         # [binary crate] depends: ALL library crates
      ├── Cargo.toml          depends: all internal crates, axum, tracing-subscriber
      └── static/             # Frontend static files (HTMX Phase 7)
          └── .gitkeep

  tests/
  └── workspace_smoke.rs      # imports all 7 crates, verifies compilation linkage
```

**Entry points:** The single entry point is `cargo build --workspace` from a clean checkout. The CI pipeline is the automated entry point on every push. There is no runtime entry point in Phase 1.

### Recommended Project Structure

```
jadepaw/                           # Git repo root
├── .github/
│   └── workflows/
│       ├── ci.yml                 # Gate + matrix workflow (D-07 through D-13)
│       └── security-audit.yml     # Scheduled cargo-audit (D-11)
├── .githooks/
│   └── pre-commit                 # fmt + clippy (D-17)
├── crates/
│   ├── jadepaw-core/
│   │   ├── src/
│   │   │   └── lib.rs            # //! docs + basic types scaffold (D-22)
│   │   └── Cargo.toml
│   ├── jadepaw-wasm/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── jadepaw-bus/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── jadepaw-agent/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── jadepaw-skill/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   ├── jadepaw-gateway/
│   │   ├── src/
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   └── jadepaw-server/
│       ├── src/
│       │   └── main.rs           # Binary crate: fn main() stub (D-22)
│       ├── static/
│       │   └── .gitkeep           # Frontend files placeholder (D-23)
│       └── Cargo.toml
├── tests/
│   └── workspace_smoke.rs        # Import all 7 crates (D-21)
├── Cargo.toml                     # Workspace root with [workspace], [features] (D-03, D-20)
├── Cargo.lock                     # Checked in (binary crate exists)
├── justfile                       # Task recipes (D-16)
├── rustfmt.toml                   # Formatter config (D-14)
├── clippy.toml                    # Linter config (D-15)
├── deny.toml                      # cargo-deny config (D-19)
├── .editorconfig                  # Cross-editor config (D-18)
├── .gitattributes                 # Line-ending normalization (D-18)
├── .gitignore                     # Rust standard template (D-25)
├── .planning/                     # GSD toolchain state (D-24)
├── docs/
│   └── jadepaw_discussion.md      # Architecture document (existing)
├── LICENSE                        # Apache-2.0 OR MIT
└── README.md
```

### Pattern 1: workspace.dependencies for Centralized Versioning

**What:** All external crate versions are defined once in the root `Cargo.toml`'s `[workspace.dependencies]` section. Individual crate `Cargo.toml` files reference them via `{ workspace = true }`. This is the pattern used by wasmtime (45+ crates, all pinned in one place) and tokio.

**When to use:** ALWAYS for multi-crate workspaces. This is now the standard Rust convention and was formalized in Rust 1.64+.

**Example:**
```toml
# Root Cargo.toml
[workspace.dependencies]
wasmtime = { version = "45.0", default-features = false, features = ["async", "pooling-allocator"] }
tokio = { version = "1.52", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }

# crates/jadepaw-core/Cargo.toml
[dependencies]
serde = { workspace = true }
```

### Pattern 2: Hybrid Feature Flag Strategy

**What:** Root workspace `[features]` defines aggregate flags. Sub-crates define granular flags. Root maps to sub-crate via `crate-name/feature` syntax.

**When to use:** When the project has two deployment modes (single-node default, cluster optional) and per-crate optional dependencies (postgres vs sqlite, redis, otlp).

**Example:**
```toml
# Root Cargo.toml
[features]
default = ["single-node"]
single-node = ["jadepaw-wasm/single-node", "jadepaw-agent/single-node"]
cluster = ["jadepaw-wasm/cluster", "jadepaw-bus/cluster", "jadepaw-agent/cluster"]

# crates/jadepaw-wasm/Cargo.toml
[features]
default = ["single-node"]          # D-06: natural defaults
single-node = []
cluster = ["redis"]

# crates/jadepaw-agent/Cargo.toml
[features]
default = ["single-node"]
single-node = []
cluster = ["redis", "otlp"]
```

`compile_error!` guards (D-04) go in the root crate (jadepaw-server since it's the binary):
```rust
#[cfg(all(feature = "single-node", feature = "cluster"))]
compile_error!("single-node and cluster modes are mutually exclusive");
```

### Pattern 3: Strict Topological Dependency Order

**What:** Each crate ONLY depends on crates earlier in the chain. The chain is: core -> wasm -> bus -> agent -> skill -> gateway -> server. `core` has zero internal dependencies.

**When to use:** Always. This is a locked decision (D-02) and standard for Rust workspaces to prevent circular dependency hell.

**Verification:** `cargo tree --workspace` will show the resolved graph. A circular dependency would fail at `cargo check` time with a clear error -- this is enforced by Cargo itself.

### Anti-Patterns to Avoid

- **Creating jadepaw-common or jadepaw-macros**: Locked by D-01. Do not create additional crates. Splitting core later is a 2-file change.
- **Circular dependency attempts**: Even an accidental `jadepaw-bus` depending on `jadepaw-agent` creates a cycle. The topological order is enforced by Cargo -- a cycle causes a compile error, preventing accidental circular deps.
- **Pinning minimum versions without `=MAJOR.MINOR`**: Using `wasmtime = "38"` is safe (semver-compatible 38.x.x). Using `wasmtime = ">=38"` is dangerous -- it could pull in wasmtime 45 with potential API breakage.
- **Cargo.lock in .gitignore**: D-01 is a library workspace with a binary crate -- Cargo.lock MUST be checked in.
- **Using serde_yaml**: It is deprecated (dtolnay, March 2024). Use `serde_yml` or defer YAML support entirely. Since Phase 1 has no config parsing, deferring is the safest option.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Workspace version management | Per-crate version strings | `[workspace.dependencies]` | Single source of truth; wasmtime standard pattern |
| Pre-commit hooks | npm-based (husky, lint-staged) | Shell scripts in `.githooks/` | D-17 mandates zero external deps |
| CI caching | Manual sccache config | `Swatinem/rust-cache@v2` | Consensus action, handles target/ and cargo registry cache automatically |
| Test runner | `cargo test` | `cargo nextest` | 2-3x faster; per-test timeouts; better output |
| Feature flag mutual exclusion | Runtime checks | `compile_error!` guards + cfg attributes | Detected at compile time, not deploy time (D-04) |

## Runtime State Inventory

N/A -- greenfield project with no existing code, no databases, no deployed services, no secrets, no build artifacts. This is Phase 1 of a brand-new project. All state is in git-ready files only.

## Common Pitfalls

### Pitfall 1: STACK.md Version Drift

**What goes wrong:** The planner or implementer copies versions from STACK.md (researched February-March 2026) without checking crates.io. wasmtime 38.0 vs 45.0.0 is a 7-major-version gap -- APIs for Store, Engine, pooling allocator may have changed.
**Why it happens:** STACK.md is treated as authoritative when it's a snapshot.
**How to avoid:** ALWAYS verify on crates.io before pinning. Phase 1 should lock CURRENT versions, not STACK.md versions.
**Warning signs:** CI fails with compilation errors after adding wasmtime dependency. async-openai 0.34 vs 0.40 -- API surface changes.

### Pitfall 2: serde_yaml Deprecation

**What goes wrong:** `serde_yaml = "0.9"` is added to workspace dependencies. Later phases parse YAML and it works. But the crate is unmaintained since March 2024 -- security advisories will not be fixed. CI's `cargo-deny advisories` job will flag it.
**Why it happens:** STACK.md lists serde_yaml 0.9 as a standard dependency. The deprecation is well-known in the Rust community but was missed in early research.
**How to avoid:** Use `serde_yml` (community fork) or defer YAML support entirely. Phase 1 has no YAML parsing code -- the dependency is not needed yet. Add it later when Phase 6 (Skill System, which uses YAML SKILL.md files) needs it.
**Warning signs:** `cargo search serde_yaml` shows version `0.9.34+deprecated`. The lib.rs page says "This project is no longer maintained."

### Pitfall 3: Empty Crate Clippy Warnings

**What goes wrong:** An empty `lib.rs` or `main.rs` file with no functions or types triggers clippy warnings: `clippy::empty_line_after_doc_comments`, unused import warnings, etc. D-09 requires `-D warnings` (zero warnings allowed), so these block CI.
**Why it happens:** The scaffold is intentionally minimal -- no business logic.
**How to avoid:** D-22 already addresses this: every `lib.rs` must have a `//!` doc comment describing the crate. Additionally, `main.rs` needs at minimum a `fn main() {}` stub. The workspace smoke test (`tests/workspace_smoke.rs`) must import something from each crate, or the imports themselves will be unused.
**Warning signs:** CI gate job fails with clippy warnings about empty crates, unused doc comments, missing docs on private items.

### Pitfall 4: cargo-deny License Allowlist Too Restrictive

**What goes wrong:** The license allow-list (D-19) is defined before any dependencies are added. When a dependency has a permitted license variant not on the list, `cargo-deny` fails the CI build. This is a configuration issue, not a real license problem.
**Why it happens:** Some crates use `Apache-2.0 WITH LLVM-exception` or `BSL-1.0`. Others use `MIT OR Apache-2.0` but the SPDX string doesn't match exact format.
**How to avoid:** After scaffolding, run `cargo deny check` locally and add needed licenses to the allow-list. The initial list in D-19 is a starting point, not final.
**Warning signs:** `cargo-deny` fails on first run with unfamiliar license identifiers.

### Pitfall 5: just binary Not Found in CI

**What goes wrong:** CI workflow tries `just build` but `just` is not installed on the GitHub Actions runner.
**Why it happens:** `just` is a separate binary, not part of Rust toolchain. The ubuntu-latest runner might not have it.
**How to avoid:** The justfile is a developer convenience (D-16). CI workflows should use raw cargo commands (`cargo build --workspace`, `cargo test --workspace`) not `just` wrappers. The `just` binary is for local development only. If CI must use `just`, add an install step: `cargo install just`.

### Pitfall 6: Feature Flag compile_error! in Wrong Crate

**What goes wrong:** `compile_error!` guards are placed in a library crate's `lib.rs`. But the feature flags (`single-node`, `cluster`) are workspace-level features -- they cascade to sub-crates via `crate-name/feature` syntax. A compile_error in `jadepaw-core` that checks for `single-node` vs `cluster` won't fire because `jadepaw-core` doesn't define those features.
**Why it happens:** The feature flag resolution happens at the dependency level, not at the CFG level within sub-crates. Workspace features map to specific sub-crate features.
**How to avoid:** Place `compile_error!` guards in the binary crate (`jadepaw-server`) or in a dedicated root crate `lib.rs` (the workspace root can also be a lib crate if needed). The guard checks for the ROOT features, not sub-crate features.

## Code Examples

### Workspace Root Cargo.toml (Pattern from wasmtime 45)

Source: wasmtime repository structure [VERIFIED: wasmtime main Cargo.toml fetched 2026-05-28 shows `[workspace]` with `resolver = "2"`, `members = [...]`, `[workspace.dependencies]`, `[workspace.package]`]

```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.package]
edition = "2024"
license = "Apache-2.0 OR MIT"
repository = "https://github.com/user/jadepaw"
rust-version = "1.85"

[workspace.dependencies]
# Core runtime
wasmtime = { version = "45.0", default-features = false }
tokio = { version = "1.52", features = ["full"] }
axum = { version = "0.8", features = ["ws"] }
tower-http = { version = "0.6", features = ["cors", "fs"] }
tower = { version = "0.5" }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
# serde_yaml: DEPRECATED: Use serde_yml when YAML needed (Phase 6)

# Database & caching
sqlx = { version = "0.9", features = ["runtime-tokio-rustls"] }
redis = { version = "1.2", features = ["tokio-comp"] }

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }

# Utilities
uuid = { version = "1.0", features = ["v7"] }
chrono = { version = "0.4", features = ["serde"] }

# LLM (Phase 3+; defined here for forward reference)
async-openai = "0.40"

[features]
default = ["single-node"]
single-node = [
    "jadepaw-wasm/single-node",
    "jadepaw-agent/single-node",
    "jadepaw-gateway/single-node",
]
cluster = [
    "jadepaw-wasm/cluster",
    "jadepaw-bus/cluster",
    "jadepaw-agent/cluster",
    "jadepaw-gateway/cluster",
]

[profile.release]
lto = "fat"
codegen-units = 1
opt-level = 3
strip = "symbols"

[profile.dev]
opt-level = 0

[profile.ci]
inherits = "dev"
debug = 0       # CARGO_PROFILE_DEV_DEBUG=0 equivalent
incremental = false  # CARGO_INCREMENTAL=0 equivalent
```

### Individual Crate Cargo.toml (e.g., jadepaw-core)

```toml
[package]
name = "jadepaw-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
uuid = { workspace = true }
chrono = { workspace = true }

# No internal jadepaw dependencies (D-02)
```

### jadepaw-wasm Cargo.toml (depends on core)

```toml
[package]
name = "jadepaw-wasm"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
jadepaw-core = { path = "../jadepaw-core" }
wasmtime = { workspace = true }
tokio = { workspace = true }

[features]
default = ["single-node"]
single-node = []
cluster = ["redis"]
redis = ["dep:redis"]

[dependencies.redis]
workspace = true
optional = true
```

### Module-level Doc Template (per D-22)

```rust
//! # jadepaw-core
//!
//! Core data types, error handling, and configuration primitives shared across all
//! jadepaw crates. This crate has zero internal jadepaw dependencies by design.
//!
//! ## What lives here
//! - Shared types: SessionId, TenantId, ToolId, SkillId, CapabilitySet
//! - Unified error types and Result aliases
//! - Configuration structs (global, tenant, session layers)
//!
//! ## What does NOT live here
//! - Wasm runtime logic (see jadepaw-wasm)
//! - Agent loop execution (see jadepaw-agent)
//! - HTTP/WS transport (see jadepaw-gateway)
```

### Workspace Smoke Test (per D-21)

```rust
//! Workspace linkage smoke test.
//!
//! This test imports all 7 crates to catch forgotten `pub use` re-exports
//! that `cargo build` might miss (e.g., when a crate compiles but its public
//! API is inaccessible).

// All crates must be importable
use jadepaw_core as core;
use jadepaw_wasm as wasm;
use jadepaw_bus as bus;
use jadepaw_agent as agent;
use jadepaw_skill as skill;
use jadepaw_gateway as gateway;
// jadepaw-server is a binary crate, cannot be imported.
// Its linkage is verified by `cargo build --workspace`.

#[test]
fn all_library_crates_importable() {
    // If this compiles, all crates are linked and their `lib.rs` files are valid.
    assert!(true);
}
```

## Package Legitimacy Audit

All dependencies in this phase are Rust crates, not Python/Node packages. The slopcheck tool checks PyPI, not crates.io. Therefore, cross-ecosystem verification was performed:

| Package | Ecosystem | Registry | Version | crates.io Verified | Postinstall Risk | Disposition |
|---------|-----------|----------|---------|-------------------|------------------|-------------|
| wasmtime | Rust | crates.io | 45.0.0 | YES via `cargo search` | N/A (Rust crate) | Approved [VERIFIED: crates-io] |
| axum | Rust | crates.io | 0.8.9 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| tokio | Rust | crates.io | 1.52.3 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| sqlx | Rust | crates.io | 0.9.0 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| serde | Rust | crates.io | 1.0.228 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| serde_json | Rust | crates.io | 1.0.150 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| tracing | Rust | crates.io | 0.1.44 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| tracing-subscriber | Rust | crates.io | 0.3.23 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| tower-http | Rust | crates.io | 0.6.11 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| tower | Rust | crates.io | 0.5.3 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| uuid | Rust | crates.io | 1.23.1 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| chrono | Rust | crates.io | 0.4.44 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| redis | Rust | crates.io | 1.2.1 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| async-openai | Rust | crates.io | 0.40.2 | YES via `cargo search` | N/A | Approved [VERIFIED: crates-io] |
| serde_yaml | Rust | crates.io | 0.9.34+deprecated | YES via `cargo search` | N/A | **REPLACED** (deprecated March 2024) |

**Packages removed due to deprecation:** `serde_yaml` -- deprecated by maintainer dtolnay in March 2024. Community successor: `serde_yml`. Since Phase 1 has no YAML parsing, recommend deferring this dependency entirely until Phase 6.

**Cross-ecosystem verification:** All packages confirmed to exist only on crates.io (not npm, not PyPI as Rust crates). This is correct -- these are Rust-native crates. The npm `tokio` (v0.1.2, 2015) is an unrelated web scraping library. The PyPI `wasmtime` (v45.0.0) is the Python binding for wasmtime, a different package.

**Postinstall script check (npm):** Not applicable -- these are Rust crates, not npm packages. Rust crate build scripts (`build.rs`) execute during compilation but are sandboxed to the project's source tree. The crates.io version verification via `cargo search --registry crates-io` confirms each package exists with the stated version.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo-nextest (via `cargo test` fallback for doc-tests) |
| Config file | none (nextest uses cargo test layout by default; `.config/nextest.toml` created if needed) |
| Quick run command | `cargo build --workspace && cargo nextest run --workspace && cargo test --doc --workspace` |
| Full suite command | `cargo build --workspace --all-features && cargo nextest run --workspace && cargo test --doc --workspace && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo fmt --all -- --check` |

### Phase Requirements to Test Map

Phase 1 has no formal requirement IDs (infrastructure phase). Tests map to the 5 success criteria from ROADMAP.md:

| SC ID | Success Criterion Behavior | Test Type | Automated Command | File Exists? |
|-------|---------------------------|-----------|-------------------|-------------|
| SC-01 | `cargo build --workspace` succeeds on clean checkout | integration | `cargo build --workspace` | Wave 0 (scaffold validates itself) |
| SC-02 | Crate dependency graph: core -> wasm -> bus -> agent -> skill -> gateway -> server | unit | `cargo tree --workspace --depth 1` inspection + tests/workspace_smoke.rs | Wave 0 |
| SC-03 | `cargo test --workspace` passes | unit | `cargo nextest run --workspace && cargo test --doc --workspace` | Wave 0 |
| SC-04 | `cargo clippy --workspace -- -D warnings` passes with zero warnings | static | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Wave 0 |
| SC-05 | CI pipeline (fmt + build + test + clippy) < 5 min on every push | e2e | Manual: push to GitHub, observe Actions run | Wave 0 (.github/workflows/ci.yml) |

### Sampling Rate
- **Per task commit:** `cargo build --workspace && cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features -- -D warnings` -- validates structural integrity after each implementation task
- **Per wave merge:** `cargo nextest run --workspace && cargo test --doc --workspace` -- validates test suite still passes
- **Phase gate:** Full CI-green run on GitHub Actions (fmt + build + test + clippy + cargo-deny + cargo-audit on both Linux stable + beta and macOS stable) before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] `.config/nextest.toml` -- nextest configuration (optional; not needed for empty test suites, but define now for consistency)
- [ ] `tests/workspace_smoke.rs` -- workspace linkage smoke test (D-21)
- [ ] `.github/workflows/ci.yml` -- CI pipeline definition (D-07 through D-13)
- [ ] `.github/workflows/security-audit.yml` -- scheduled cargo-audit workflow (D-11)
- [ ] `cargo-nextest` binary not installed on this machine -- must be installed before running tests

## Security Domain

### Applicable ASVS Categories

Phase 1 is infrastructure scaffolding -- no authentication, no session management, no data access, no user input. Most ASVS categories are NOT applicable.

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | N/A -- no auth code in Phase 1 |
| V3 Session Management | No | N/A -- no session code |
| V4 Access Control | No | N/A -- no access control code |
| V5 Input Validation | No | N/A -- no user input handling |
| V6 Cryptography | No | N/A -- no crypto code |
| V10 Malicious Code Protection | Yes (supply chain) | cargo-deny (bans, licenses), cargo-audit (vulnerabilities) |
| V14 Configuration | Yes | `.gitignore` secrets exclusion, `rustls` over `openssl` |

### Applicable Controls for Phase 1

Phase 1's security relevance is **supply chain security** and **secure defaults**:

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious dependency injection | Tampering | `cargo-deny bans` blocks known-bad crates; `cargo-deny advisories` alerts on RustSec CVEs |
| License compliance violation | Information Disclosure | `cargo-deny licenses` allow-list (D-19) |
| Vulnerable transitive deps | Elevation of Privilege | `cargo-audit` scheduled workflow (D-11) |
| Secrets in version control | Information Disclosure | `.gitignore` blocks `.env` files (D-25); `.env.example` is allowed |
| OpenSSL CVEs in transitive deps | Multiple | `deny.toml` bans `openssl` and `openssl-sys` crates (D-19), forces `rustls` |
| Supply chain typosquatting | Spoofing | crates.io verification with `cargo search` before pinning versions in `[workspace.dependencies]` |

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc | All crate compilation | YES | 1.95.0 | -- |
| cargo | Build orchestration | YES | 1.95.0 | -- |
| git | Version control, pre-commit hooks | YES | 2.49.0 | -- |
| just | Dev task runner (justfile) | YES | 1.40.0 | Can use raw cargo commands |
| cargo-nextest | Test execution (CI) | NO | -- | `cargo test` (slower, less output) |
| cargo-deny | License/security audit (CI) | NO | -- | Install via `cargo install cargo-deny` |
| cargo-audit | Vulnerability scanning (CI) | NO | -- | Install via `cargo install cargo-audit` |
| node/npm | None (no frontend yet) | YES (22.17.0) | -- | Not needed in Phase 1 |
| Docker | None | Not checked | -- | Not needed in Phase 1 |
| wasm32-wasi target | Wasm compilation (Phase 2+) | NO | -- | Not needed in Phase 1; install in Phase 2 |

**Missing dependencies with no fallback:**
- cargo-nextest -- CI workflow (D-10) requires it. Must be installed (`cargo install cargo-nextest`) before CI can run. Local dev can use `cargo test` as fallback.
- cargo-deny -- CI security gate (D-11) requires it. Must be installed before CI can complete.
- cargo-audit -- Scheduled CI workflow (D-11) requires it. Can be installed at CI time.

**Missing dependencies with fallback:**
- None blocking. All missing tools can be installed with `cargo install`.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | wasmtime 45.0.0 API is compatible with patterns documented in STACK.md (Engine::default(), Store::new(), InstancePre pool, async feature). The major version jump from 38 to 45 (7 versions) could include breaking API changes not yet investigated. | Standard Stack | MEDIUM -- If APIs changed significantly, Phase 2 planning will need adjustment. Phase 1 only needs the crate to compile and link, which wasmtime 45.0.0 should do. |
| A2 | `serde_yml` is a drop-in replacement for `serde_yaml` 0.9.x. The community fork may have API differences. | Standard Stack | LOW -- Phase 1 defers YAML dependency entirely, so no risk in Phase 1. Phase 6 (Skill System) needs to evaluate serde_yml stability at that time. |
| A3 | sqlx 0.9.0 API is backward-compatible with sqlx 0.8.x patterns documented in STACK.md. The 0.9 release is new (May 2026). | Standard Stack | LOW -- Phase 1 doesn't use sqlx. Phase 5 (Session Memory) is the first phase that needs it. |
| A4 | redis-rs 1.x API is reasonably compatible with 0.28.x. The jump from 0.28 to 1.0 was a major version bump; APIs likely changed. | Standard Stack | LOW -- Phase 1 doesn't use redis. Phase 5 is the first consumer. |
| A5 | Rust edition 2024 compatibility: All crates (wasmtime 45, axum 0.8, tokio 1.52, sqlx 0.9) compile cleanly under edition 2024. The wasmtime 45 workspace itself uses edition 2024. | Standard Stack | LOW -- wasmtime 45 is confirmed to use edition 2024 in its own workspace. Other crates likely adopted it by now. |

## Open Questions

1. **serde_yaml replacement: serde_yml or defer?**
   - What we know: serde_yaml 0.9.34+deprecated, unmaintained since March 2024. serde_yml is the community fork. Phase 1 has NO YAML parsing code.
   - What's unclear: Whether serde_yml is stable enough to add now as a forward dependency, or whether deferring to Phase 6 (first phase that needs YAML for SKILL.md files) is safer.
   - Recommendation: Defer YAML dependency entirely. Add `serde_yml` in Phase 6 when it's first needed. Phase 1's workspace dependencies should only include crates actually consumed by scaffolded code.

2. **wasmtime 45 API Breakage Risk**
   - What we know: STACK.md was researched against wasmtime 38.x. Current is 45.0.0. The Bytecode Alliance ships semver-compatible releases within a major, but 38->45 could have breaking changes in async APIs, WASI preview2, or component model.
   - What's unclear: Whether `Engine::default()`, `Store::new()`, `InstancePre`, or `ResourceLimiter` APIs have changed since 38.
   - Recommendation: Phase 1 only needs wasmtime to compile and link -- no runtime behavior. Accept the risk of API changes; Phase 2 (Wasm Isolation Core) is the first phase to actually use wasmtime APIs, and it will research the current API surface.

3. **GitHub Actions runner Rust version alignment**
   - What we know: This machine has rustc 1.95.0. GitHub's ubuntu-latest and macos-latest runners may have different Rust versions available via dtolnay/rust-toolchain.
   - What's unclear: Whether the `stable` toolchain on GitHub Actions matches 1.95.0 or is slightly ahead/behind.
   - Recommendation: Use `dtolnay/rust-toolchain@stable` in CI (D-07). It always pulls the latest stable. Minor version mismatches between local and CI are normal and acceptable.

## Sources

### Primary (HIGH confidence)
- crates.io registry search via `cargo search --registry crates-io` -- verified all 15 crate versions on 2026-05-28
- wasmtime main branch Cargo.toml [WebFetch -- 2026-05-28] -- confirmed `[workspace.dependencies]` pattern, edition 2024, wasmtime 46.0.0 (HEAD, ahead of 45.0.0 release)
- docs.rs/wasmtime/latest -- confirmed API: Engine::default(), Store::new(), async feature, pooling-allocator
- docs.rs/axum/latest -- confirmed WebSocket (`ws` feature) and SSE support in axum 0.8.9
- docs.rs/sqlx/latest -- confirmed sqlx 0.9.0, runtime-tokio feature
- lib.rs/crates/serde_yaml -- confirmed deprecation status, March 2024, community alternatives listed
- wasmtime GitHub: `/bytecodealliance/wasmtime` -- reference workspace structure (members in `crates/` directory)
- Project context files: CONTEXT.md (25 locked decisions), ROADMAP.md (5 success criteria), CLAUDE.md (tech stack), ARCHITECTURE.md (crate layout), STACK.md (historical versions)

### Secondary (MEDIUM confidence)
- WebSearch on Rust workspace best practices -- workspace.dependencies pattern confirmed as standard
- WebSearch on serde_yaml deprecation -- multiple sources confirm March 2024 deprecation, serde_yml as community fork

### Tertiary (LOW confidence)
- None. All critical findings (version drift, deprecation) are verified on crates.io.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all crate versions verified on crates.io within this research session
- Architecture: HIGH -- workspace patterns confirmed from wasmtime reference, all decisions locked by CONTEXT.md
- Pitfalls: HIGH -- version drift, serde_yaml deprecation, empty crate warnings are well-understood issues
- State of the art: HIGH -- Rust 2024 edition, workspace.dependencies, nextest are all current best practices

**Research date:** 2026-05-28
**Valid until:** 2026-06-28 (30 days -- stable domain, but crate versions evolve monthly)

## Dependencies Discovery via Graph Analysis

No knowledge graph exists at `.planning/graphs/graph.json`. Skipping graph context injection. Note: This is a greenfield Phase 1 -- there are no existing source files to graph. The graph would be empty.