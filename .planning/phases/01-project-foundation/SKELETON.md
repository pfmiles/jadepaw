# Walking Skeleton — jadepaw

**Phase:** 1
**Generated:** 2026-05-28

## Capability Proven End-to-End

A developer clones the repo, and `cargo build --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings` all pass. Pushing to GitHub triggers a CI pipeline that runs fmt + build + test + clippy in under 5 minutes on Linux (stable + beta) and macOS (stable).

## Architectural Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Framework | Rust 2024 edition + Cargo workspace | Project constraints mandate Rust + wasmtime + tokio. Cargo workspace with `crates/` subdirectory (wasmtime pattern) manages the 7-crate dependency graph. |
| Crate structure | 7 crates in strict topological order: core -> wasm -> bus -> agent -> skill -> gateway -> server | D-01, D-02. Each crate depends only on earlier crates. Core has zero internal deps. Server is the binary crate wiring everything together. |
| Version management | `[workspace.dependencies]` in root Cargo.toml | Centralized version pinning per wasmtime 45+ standard. All crates reference deps via `{ workspace = true }`. Single source of truth prevents version drift across 7 crates. |
| Feature flags | Hybrid: root workspace features (`single-node` default, `cluster`) map to sub-crate features via `crate-name/feature` syntax | D-03. Per-crate features (`sqlite`, `redis`, `otlp`) with natural defaults (D-06). `compile_error!` guard in jadepaw-server/src/lib.rs prevents mutually exclusive feature combinations (D-04). |
| Data layer | SQLx 0.9 (SQLite for single-node, PostgreSQL for cluster) | Compile-time checked SQL, no ORM overhead. SQLite default for zero-config single-node. PostgreSQL for cluster mode. |
| LLM client | async-openai 0.40 with `Box<dyn Config>` | Runtime provider dispatch (D-05). No compile-time LLM feature flags. OpenAI-compatible API supports OpenAI, Azure, Ollama, DeepSeek, Groq. |
| HTTP framework | axum 0.8 + tower-http 0.6 | Built on tokio + tower + hyper. Native SSE/WebSocket support. Tower middleware ecosystem for auth, tracing, compression. |
| Frontend | HTMX 2.0 + SSE (static files in crates/server/static/) | Zero-build hypermedia library. No npm build step. Served by axum via tower-http ServeDir. Deferred to Phase 7 — directory created now as placeholder. |
| Observability | tracing 0.1 + tracing-subscriber 0.3 | Structured spans + events model maps to agent session lifecycle. Deferred OTLP export (cluster mode only). Metrics + Prometheus exporter in Phase 9. |
| Cache / session store | redis-rs 1.2 (cluster mode only) | Redis optional via feature gate. In-memory fallback for single-node mode. |
| Serialization | serde 1.0 + serde_json 1.0 | Standard Rust serialization. serde_yaml DEPRECATED — deferred entirely to Phase 6 (serde_yml). |
| Test runner | cargo-nextest | 2-3x faster than cargo test. Per-test timeouts. Better output formatting. |
| CI platform | GitHub Actions | dtolnay/rust-toolchain + Swatinem/rust-cache@v2. Gate job (fmt+clippy+doc) runs first; test matrix (Linux stable+beta, macOS stable) runs after. |
| Security audit | cargo-deny (licenses + bans) + cargo-audit (RustSec advisories) | `deny.toml` bans openssl/openssl-sys (forces rustls). License allow-list per D-19. cargo-audit runs weekly + on Cargo.lock changes. |
| Code style | rustfmt style_edition = "2024", clippy pedantic with targeted allows | D-14, D-15. Max width 100 columns. Group imports StdExternalCrate. No nursery/restriction lints. |
| Pre-commit | Shell script in .githooks/ (zero external deps) | D-17. Runs `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`. |
| Task runner | just (justfile) | D-16. Recipes: build, test, lint, fmt, deny, audit, wasm-build, etc. CI uses raw cargo commands. |
| Deployment target | Local `cargo build --workspace` | Phase 1 proves build succeeds from clean checkout. Runtime deployment deferred to Phase 7 (web server). |
| Directory layout | `crates/` subdirectory (wasmtime pattern) | D-20. Workspace members = ["crates/*"]. Tests at `tests/` (integration). `.config/` for tool configs. `.github/workflows/` for CI. |
| Auth | Not applicable (Phase 1 infrastructure) | Deferred to Phase 7/8 (API Key + JWT, tower middleware). |
| Database migrations | Not applicable (Phase 1 infrastructure) | SQLx migrations deferred to Phase 5 (Session Memory). |
| Wasm runtime | wasmtime 45.0 (not 38.0 from STACK.md) | Pinned to current crates.io version as of 2026-05-28. 7 major releases ahead of STACK.md — APIs may have changed. Phase 2 (Wasm Isolation Core) validates API compatibility. |

## Stack Touched in Phase 1

- [x] Project scaffold — Cargo workspace with 7 crates, `crates/` layout, root Cargo.toml with workspace.dependencies
- [x] Build — `cargo build --workspace` succeeds from clean checkout
- [x] Test runner — `cargo nextest run --workspace` passes (workspace smoke test)
- [x] Lint — `cargo clippy --workspace --all-targets --all-features -- -D warnings` passes with zero warnings
- [x] Format — `cargo fmt --all -- --check` passes
- [x] Documentation — `cargo doc --workspace --no-deps --document-private-items` builds
- [x] CI — GitHub Actions pipeline (gate + matrix) runs on every push
- [x] Security — cargo-deny (bans+licenses) in CI gate, cargo-audit scheduled workflow
- [x] Pre-commit — Shell script hook runs fmt+clippy before each commit
- [x] Task runner — justfile with recipes for all common tasks

## Out of Scope (Deferred to Later Slices)

- Wasm instance creation and pool management (Phase 2)
- Agent ReAct loop and LLM integration (Phase 3)
- Tool execution and sandboxing (Phase 4)
- Session memory and persistence (Phase 5)
- Skill system with YAML frontmatter (Phase 6 — serde_yml added here)
- Web chat UI (Phase 7)
- Skill management UI (Phase 8)
- Prometheus metrics and OTLP tracing (Phase 9)
- Database migrations and SQLx query files
- OAuth / JWT authentication
- Redis cluster and NATS message bus
- Docker containers and deployment infrastructure
- Code coverage (cargo-tarpaulin — deferred to Phase 2 per D-12)
- serde_yaml / serde_yml (deferred to Phase 6 — YAML not needed before Skill System)
- wasm32-wasi target installation (needed Phase 2)
- HTMX / Alpine.js frontend files (needed Phase 7)

## Subsequent Slice Plan

Each later phase adds one vertical slice on top of this skeleton without altering its architectural decisions:

- Phase 2: Create isolated Wasm Store per session, enforce resource limits, validate capability whitelist
- Phase 3: Natural language messages to agent produce streaming ReAct responses with safety limits
- Phase 4: Agent uses external tools (file read/write, HTTP) via MCP-compatible protocol
- Phase 5: Conversations persist within sessions with context window management and resume capability
- Phase 6: Declarative SKILL.md files load at runtime, swapping skills changes agent behavior instantly
- Phase 7: Browser-based streaming chat at localhost:PORT via HTMX + SSE
- Phase 8: Web UI for listing, loading, and unloading skills
- Phase 9: Session-correlated tracing and Prometheus metrics endpoint at /metrics