# Phase 1: Project Foundation - Context

**Gathered:** 2026-05-28
**Status:** Ready for planning

<domain>
## Phase Boundary

This phase creates the structural skeleton that all other phases build upon: a Rust workspace with 7 crates in dependency order, build system configuration, and a CI pipeline that runs fmt + build + test + clippy on every push in under 5 minutes. No business logic — pure infrastructure scaffold.

**Success Criteria (from ROADMAP.md):**
1. `cargo build --workspace` succeeds from clean checkout on macOS and Linux
2. Crate dependency graph matches: core → wasm → bus → agent → skill → gateway → server
3. `cargo test --workspace` passes
4. `cargo clippy --workspace -- -D warnings` passes with zero warnings
5. CI pipeline (fmt + build + test + clippy) runs on every push and completes in under 5 minutes

</domain>

<decisions>
## Implementation Decisions

### Crate Structure
- **D-01:** Exactly 7 crates as documented in ROADMAP.md: `jadepaw-core` → `jadepaw-wasm` → `jadepaw-bus` → `jadepaw-agent` → `jadepaw-skill` → `jadepaw-gateway` → `jadepaw-server`. No additional `jadepaw-common` or `jadepaw-macros` crate at scaffold time — splitting core or adding a macros crate later is a 2-file change, not architectural rework.
- **D-02:** Dependency graph is strict topological order. Each crate only depends on crates earlier in the chain. Zero circular dependencies. `jadepaw-core` has no internal jadepaw dependencies.

### Feature Flags
- **D-03:** Hybrid strategy — root `Cargo.toml` `[features]` table defines aggregate features (`cluster`, `single-node` as default). Sub-crates define per-crate features (e.g., `postgres`, `sqlite`, `redis`, `otlp`) with `#[cfg]` gates. Root features map to sub-crate features via `crate-name/feature` syntax.
- **D-04:** `compile_error!` guards in root crate prevent mutually exclusive features (e.g., `postgres` + `sqlite` simultaneously).
- **D-05:** LLM providers remain fully runtime via `Box<dyn Config>` pattern — never become feature flags.
- **D-06:** Sub-crates must be independently buildable/testable with natural defaults (`sqlite` for database, in-memory for cache, no OTLP).

### CI Pipeline
- **D-07:** GitHub Actions with the Rust CI consensus stack. Caching: `Swatinem/rust-cache@v2` (with `cache-bin: false` on macOS). Toolchain: `dtolnay/rust-toolchain`.
- **D-08:** Matrix: Linux (ubuntu-latest) stable + beta, macOS (macos-latest) stable only.
- **D-09:** Fast gate job (`check`) runs first: `cargo fmt --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo doc --workspace --no-deps --document-private-items`. Test matrix runs in parallel after gate passes.
- **D-10:** Test runner: `cargo nextest run --workspace` + `cargo test --doc --workspace`.
- **D-11:** Security: `cargo-deny` (bans + licenses blocking, advisories non-blocking via `continue-on-error`). `cargo-audit` in separate scheduled workflow (weekly + on Cargo.lock changes).
- **D-12:** Code coverage deferred to Phase 2 (no code to cover at scaffold stage; would break <5 min CI target).
- **D-13:** CI speed optimizations: `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, cancel-in-progress per PR/branch.

### Dev Tooling & Conventions
- **D-14:** rustfmt: `style_edition = "2024"`, `group_imports = "StdExternalCrate"`, `imports_granularity = "Crate"`, `max_width = 100`.
- **D-15:** Clippy: `pedantic = "warn"` with targeted allows: `similar_names`, `module_name_repetitions`, `cast_precision_loss`, `unreadable_literal`. Nursery and restriction groups NOT enabled.
- **D-16:** Task runner: `just` (justfile) with recipes for `build`, `test`, `lint`, `fmt`, `deny`, `audit`, `wasm-build`.
- **D-17:** Pre-commit hooks: Custom shell scripts in `.githooks/` (zero external deps). `pre-commit` hook runs `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`. Configured via `git config core.hooksPath .githooks`.
- **D-18:** `.editorconfig` (consistent indentation/line endings) and `.gitattributes` (line-ending normalization, `export-ignore` for `target/`) included from Day 1.
- **D-19:** cargo-deny: license allow-list = Apache-2.0, MIT, ISC, BSD-2-Clause, BSD-3-Clause, Unicode-3.0, Zlib. Ban `openssl`/`openssl-sys` in favor of `rustls`. Duplicate dependency detection enabled.

### Project Initialization Structure
- **D-20:** Crate layout: `crates/` subdirectory (wasmtime pattern). Workspace `members = ["crates/*"]`.
- **D-21:** Workspace-level smoke test in `tests/` that imports all 7 crates — catches forgotten `pub use` re-exports that `cargo build` would miss.
- **D-22:** Each crate's `src/lib.rs` includes a `//!` module-level doc comment describing what the crate owns. Prevents empty-crate clippy warnings. Serves as architecture onboarding.
- **D-23:** Frontend static files directory: `crates/server/static/` created now with `.gitkeep`. `ServeDir::new("static")` resolves naturally when running from workspace root.
- **D-24:** `.planning/` directory kept visible in git (not gitignored) — it's the GSD toolchain's source of truth for project state.
- **D-25:** `.gitignore`: Rust standard template — `target/`, `*.rs.bk`, `.env` (but not `.env.example`), IDE dirs (`.vscode/`, `.idea/`), OS files (`.DS_Store`).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Architecture & Design
- `docs/jadepaw_discussion.md` — Wasm isolation model, instance pool design, security model, component architecture
- `docs/arch.mermaid` — Architecture diagram (Mermaid format), visual component layout
- `.planning/research/ARCHITECTURE.md` — System overview, component responsibilities, per-crate file tree proposals
- `.planning/research/STACK.md` — Complete technology stack with versions, "Stack Patterns by Variant" (single-node vs cluster), installation commands
- `.planning/notes/mvp-core-decisions.md` — MVP core decisions: Agent Loop design (hybrid mode), Skill bootstrapping, Skill format

### Requirements & Roadmap
- `.planning/REQUIREMENTS.md` — 16 v1 requirements across 7 domains, traceability matrix mapping to phases
- `.planning/ROADMAP.md` §Phase 1 — Phase goal, success criteria (5 items), dependency chain (core→wasm→bus→agent→skill→gateway→server)
- `.planning/PROJECT.md` — Core value, constraints (tech stack, isolation, deployment density, multi-tenancy), key decisions

### Project State
- `.planning/STATE.md` — Current position (Phase 1, not started), performance targets (build <5 min, cold start <5ms, 10000 instances)

### Reference Implementations
- wasmtime CI configuration (`.github/workflows/`) — Reference for CI structure, caching, matrix strategy
- tokio workspace structure — Reference for crate layout and feature flag patterns
- axum CI configuration — Reference for clippy version pinning, test matrix approach

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- None — greenfield project, no code written yet.

### Established Patterns
- None yet — conventions will be established by this phase.

### Integration Points
- All subsequent phases depend on this phase's workspace scaffold and CI pipeline.
- Phase 2 (Wasm Isolation Core) needs `jadepaw-core` and `jadepaw-wasm` crates with wasmtime dependency configured.
- Phase 7 (Web Chat UI) needs `crates/server/static/` directory for HTMX files.

</code_context>

<specifics>
## Specific Ideas

No specific references or examples were mentioned during discussion. Open to standard approaches informed by the reference implementations (wasmtime, tokio, axum).

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 1-Project Foundation*
*Context gathered: 2026-05-28*