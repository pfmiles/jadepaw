<!-- GSD:project-start source:PROJECT.md -->
## Project

**jadepaw**

jadepaw 是一个可直接被最终用户使用的通用 AI Agent 引擎。核心理念是将"Skill"视为自然语言程序——用户无需传统编程能力，通过自然语言即可定制 Agent 行为，并可一键将个人创作的 Agent 发布为多租户企业级服务。底层基于 WebAssembly 实现强隔离、高密度的多租户架构。

**Core Value:** 让任何人都能用自然语言"编程"自己的 AI Agent，并将它部署为可供成百上千人同时使用的企业级服务。

### Constraints

- **Tech stack**: Rust + wasmtime + tokio。不可变更的核心组合
- **Isolation**: Wasm 线性内存模型提供硬件级隔离，不允许退化为进程级隔离
- **Deployment density**: 单机 ≥10000 活跃实例，冷启动 ≤5ms P99
- **Multi-tenancy**: 从 Day 1 就设计为多租户架构，不能后期打补丁
- **Interface**: 内置 Web 服务器统一本地和远程 UI，不做单独的 CLI 或桌面 App
- **License**: 开源项目，周期自由，质量优先于速度
<!-- GSD:project-end -->

<!-- GSD:stack-start source:research/STACK.md -->
## Technology Stack

## Recommended Stack
### Core Technologies
| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| **Rust** | 1.85+ (2024 edition) | Host language | Zero-cost abstractions, compile-time memory safety, official wasmtime support. The project constraints mandate Rust. |
| **wasmtime** | 38.0+ | WebAssembly runtime | Bytecode Alliance project. Official Rust bindings. Pooling allocator for sub-ms instance creation. Supports WASI preview1 and preview2 (component model). Has a formal security vulnerability disclosure process. Cranelift JIT compiler is the fastest option. |
| **tokio** | 1.43+ | Async runtime | Mature, high-performance, industry standard for Rust async. Required by axum. Multi-threaded work-stealing scheduler fits high-concurrency agent workloads. |
| **axum** | 0.8.4 | Web framework | Built on tokio + tower + hyper. Native SSE support for LLM streaming. Built-in WebSocket support. Extractors for state, headers, JSON. Tower middleware ecosystem for auth, tracing, compression. Officially maintained by the tokio team. |
| **async-openai** | 0.34.0 | LLM client | Official-looking async bindings for OpenAI-compatible APIs. Native SSE streaming support. `Config` trait with `Box<dyn Config>` enables dynamic provider dispatch at runtime (OpenAI, Azure, Ollama, DeepSeek, Groq, any OpenAI-compatible endpoint). Configurable HTTP client and backoff. |
| **HTMX** | 2.0.4 | Frontend framework | Zero-build hypermedia library. SSE extension (`sse-connect`, `sse-swap`) for real-time LLM token streaming into the DOM. WebSocket support for bidirectional chat. Embeddable as static files served by axum via `tower-http` `ServeDir`. No npm build step required. |
| **SQLx** | 0.8+ | Database access | Async-native, compile-time query checking. Supports PostgreSQL (primary), SQLite (single-node mode), MySQL. No ORM overhead -- direct SQL with type safety. Migrations built in. |
| **redis-rs** | 0.28+ | Cache / session store | Async Redis client with connection pooling and cluster support. Session state storage, distributed locks, pub/sub for cross-node messages. |
| **tracing** | 0.1+ | Structured logging & spans | De facto Rust observability framework. Spans + events model maps perfectly to agent session lifecycle. Integrates with OpenTelemetry for distributed tracing. |
| **opentelemetry-rust** | 0.28+ | Metrics & trace export | OTLP exporter to Jaeger/Tempo/Grafana. OTLP-based metrics export to Prometheus. Connects agent sessions across host nodes in cluster mode. |
| **metrics** | 0.24+ | Application metrics | Lightweight facade for counters, gauges, histograms. `metrics-exporter-prometheus` for `/metrics` endpoint. Tracks Wasm instance count, request latency, LLM token usage. |
| **serde** / **serde_json** | 1.0+ | Serialization | Standard for Rust. Required by virtually every other crate. Used for config parsing (YAML via serde_yaml), API request/response, Wasm guest-host message serialization. |
| **tower-http** | 0.6+ | HTTP middleware | `CorsLayer`, `CompressionLayer`, `TraceLayer`, `ServeDir` (static file serving for HTMX frontend), `AuthLayer` integration point. |
### Supporting Libraries
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **wasmtime-wasi** | 38.0+ | WASI host implementation | Provides `WasiCtx` and `WasiView` for WASI preview1/preview2. Required for file system and stdio access from guest Wasm modules. |
| **axum-test** | 0.17+ | End-to-end HTTP/WS testing | Test axum routers including WebSocket upgrade flow and SSE stream verification. Avoids spinning up real TCP sockets. |
| **rstest** | 0.24+ | Parameterized tests | Fixture-based test framework. Parameterized tests for LLM provider compatibility, Wasm module validation. `#[future]` for async fixtures. `#[awt]` for auto-await. |
| **testcontainers-rs** | 0.24+ | Integration test infrastructure | Spin up Redis, PostgreSQL in Docker for integration tests. `AsyncRunner` for tokio-based tests. Automatic cleanup via RAII. |
| **tower** | 0.5+ | Service abstraction | Used indirectly via axum. Direct use needed for custom middleware (auth, tenant routing, rate limiting). `ServiceBuilder` for composing layers. |
| **serde_yaml** | 0.9+ | YAML config parsing | Skill definitions, agent configurations, tenant settings all use YAML per the architecture doc. |
| **uuid** | 1.0+ | ID generation | Session IDs, instance IDs, tenant IDs, request tracing IDs. Type 7 (time-ordered) for database index friendliness. |
| **chrono** or **time** | 0.4+ | Timestamp handling | Audit log timestamps, session expiry, cron scheduling. `time` is lighter but `chrono` has wider ecosystem support. |
| **tracing-subscriber** | 0.3+ | Log formatting & filtering | `fmt` layer for human-readable console output, `EnvFilter` for dynamic log levels, JSON layer for structured logging to file. |
| **tracing-opentelemetry** | 0.28+ | Trace→OTLP bridge | Connects `tracing` spans to OpenTelemetry exporters. Each agent session maps to a root span with session_id attribute. |
| **metrics-exporter-prometheus** | 0.16+ | Prometheus metrics endpoint | Exposes `/metrics` for Prometheus scraping. Instance count, memory usage, request latency histograms, LLM token counts. |
### Development Tools
| Tool | Purpose | Notes |
|------|---------|-------|
| **cargo-nextest** | Test runner | Faster than `cargo test`. Better output formatting. Parallel test execution with per-test timeout. Essential for large Wasm test suites. |
| **cargo-deny** | License & security audit | Checks dependency licenses, bans known-vulnerable crate versions, detects duplicate dependencies. CI required. |
| **cargo-audit** | Vulnerability scanning | Checks against RustSec advisory database. Wasmtime is a security-critical dependency -- run on every PR. |
| **wasm-pack** | Wasm guest SDK | Build Wasm modules from Rust guest code. Not needed for runtime (host), but for building example/test guest modules. Use `rustc` with `wasm32-wasi` target directly for production guest builds. |
| **Wasmtime CLI** | wasmtime 38.0+ | Debugging: run `.wasm` files outside the platform for REPL-style testing. Profile guest modules before embedding. |
## Installation
# Core runtime dependencies
# Database & caching
# Observability
# Serialization & config
# HTTP middleware & utilities
# Dev dependencies
### Cargo.toml Profile for Release
## Alternatives Considered
| Recommended | Alternative | Why Not Alternative |
|-------------|-------------|---------------------|
| **wasmtime 38** | wasmer | Slower release cadence. Weaker security vulnerability disclosure process. Lower benchmark scores on Context7 (15 vs 61.5). Community momentum significantly favors wasmtime (Bytecode Alliance backing). |
| **wasmtime 38** | wasm3 | Interpreted-only (no JIT). Designed for embedded/IoT, not server workloads. No pooling allocator. Cannot achieve sub-5ms cold start with JIT compilation. |
| **axum 0.8** | actix-web | Actor model adds unnecessary complexity. axum's extractor model is more ergonomic for session/tenant state passing. axum is the tokio team's official framework. actix-web uses its own runtime (not tokio), creating ecosystem friction. |
| **axum 0.8** | warp | Maintenance has slowed. Smaller community. Filter-based composition is harder to debug than axum's Router-based approach. Less native SSE support. |
| **axum 0.8** | poem | Newer, smaller community. Good OpenAPI support but agent platforms don't need OpenAPI docs. Axum has wider middleware ecosystem via tower. |
| **async-openai 0.34** | Manual reqwest + SSE parsing | Massive reinvention. async-openai handles token streaming, error retries, provider dispatch, request/response types. Writing SSE streaming manually is error-prone. |
| **async-openai 0.34** | langchain-rust | Over-engineered for jadepaw's needs. The project builds its own Agent Loop. langchain-rust adds abstraction layers we would fight against. We need the API client, not a framework. |
| **HTMX 2.0** | React/Vue/Svelte SPA | Requires npm build, bundle step, API layer duplication. For a chat/Skill management UI, HTMX + SSE achieves the same UX with 10x less code. jadepaw is Web-first, not SPA-first. |
| **HTMX 2.0** | Vanilla JS only | HTMX is 14KB gzipped and handles SSE reconnection, DOM swapping, WebSocket management that you'd otherwise write manually. Small enough to not be a dependency risk. |
| **SQLx 0.8** | SeaORM / Diesel | ORMs add overhead for agent state patterns (JSONB-heavy, dynamic schemas). SQLx provides compile-time checked raw SQL -- exactly what we need for session state, audit logs, tenant configs. |
| **tracing + OTel** | log + env_logger | `log` crate is unstructured. `tracing` spans map directly to agent session lifecycles. OpenTelemetry is mandatory for cluster-mode distributed tracing across host nodes. |
## What NOT to Use
| Avoid | Why | Use Instead |
|-------|-----|-------------|
| **wasm-bindgen** (guest side) | Designed for wasm-browser interop, not wasm-host interop. Adds JS shim complexity. | Use raw `wit-bindgen` or simple `extern "C"` FFI for guest-host communication. The architecture uses Host Functions, not JS bridge. |
| **warp** | Effectively in maintenance mode. Filter-based routing is hard to compose for complex agent API surfaces. | axum 0.8 |
| **actix-web** | Uses its own async runtime (actix-rt), not tokio. Every other dependency (wasmtime async, async-openai, redis, sqlx) uses tokio. Mixing runtimes causes pain. | axum 0.8 |
| **reqwest** (for LLM streaming) | You'd need to manually parse SSE events, handle `data: [DONE]`, manage reconnection. async-openai + tokio handles all of this. | async-openai 0.34 |
| **langchain-rust** | Framework, not a library. Imposes its own agent architecture. jadepaw builds a custom Agent Loop with Wasm isolation -- langchain would fight every design decision. | async-openai (API client) + custom Agent Loop |
| **Diesel ORM** | Synchronous by default. Requires schema.rs generation. Doesn't handle the dynamic JSONB patterns agent state needs. | SQLx 0.8 |
| **Alpine.js** (for chat) | Adds state management (x-data, x-model) that is overkill for a chat UI. HTMX alone handles the SSE streaming pattern more cleanly for this use case. | HTMX 2.0 (Alpine.js is fine for the Skill management dashboard with forms, but not needed for the chat core) |
| **`with_default_exporter()`** (opentelemetry) | Deprecated global provider. Creates global state that breaks in multi-tenant testing scenarios. | Explicit `SdkTracerProvider` / `SdkMeterProvider` with manual setup |
| **`tower::limit::ConcurrencyLimitLayer`** (for tenant quotas) | Global limit per route, not per tenant. | Custom middleware using `DashMap<TenantId, Semaphore>` for per-tenant concurrency control |
| **`#[tokio::test]`** (for Wasm component tests) | Single-threaded by default unless `flavor = "multi_thread"`. Wasm instantiation tests benefit from parallelism. | `#[tokio::test(flavor = "multi_thread")]` always, or use `#[rstest]` + `#[tokio::test(flavor = "multi_thread")]` |
## Stack Patterns by Variant
- Use SQLite (via SQLx) instead of PostgreSQL for zero-config deployment
- Use `tower-http::fs::ServeDir` for embedded static files instead of CDN
- Use in-memory rate limiter (`tokio::sync::Semaphore`) instead of Redis-based
- No OTLP exporter needed -- `tracing-subscriber` with JSON file output suffices
- Use PostgreSQL for persistent state (tenant configs, skill definitions, audit logs)
- Use Redis for session state cache, distributed locks, pub/sub for cross-node messages
- Use OTLP exporter to Jaeger/Tempo for distributed tracing across host nodes
- Use S3/MinIO for tool output storage, Wasm module storage
- Use `tower_http::cors::CorsLayer` for cross-origin Web UI access
- Use `OpenAIConfig::new().with_api_base("http://localhost:11434/v1")` -- the OpenAI-compatible API is the standard
- Set longer timeouts via `reqwest::ClientBuilder::timeout()` since local inference is slower
- Consider `backoff::ExponentialBackoff` with longer max elapsed time
- Use the standard `OpenAIConfig` or `AzureConfig`
- For non-OpenAI providers (DeepSeek, Groq, Together), use `OpenAIConfig::new().with_api_base(...)` as they all implement OpenAI-compatible APIs
- For Anthropic (non-OpenAI-compatible), wrap with a local proxy (e.g., LiteLLM) or use a separate Anthropic-specific crate for the minority case
## Version Compatibility
| Package A | Must Work With | Verified |
|-----------|---------------|----------|
| wasmtime 38.x | tokio 1.43+ | Yes -- wasmtime's async support uses tokio |
| axum 0.8.x | tokio 1.43+ | Yes -- same team, tight integration |
| axum 0.8.x | tower 0.5.x | Yes -- axum depends on tower |
| axum 0.8.x | tower-http 0.6.x | Yes -- same tokio ecosystem |
| async-openai 0.34.x | tokio 1.43+ | Yes -- requires tokio |
| async-openai 0.34.x | reqwest 0.12+ (transitive) | Yes -- configurable HTTP client |
| sqlx 0.8.x | tokio 1.43+ | Yes -- `runtime-tokio-rustls` feature |
| redis 0.28.x | tokio 1.43+ | Yes -- `tokio-comp` feature |
| tracing-opentelemetry 0.28.x | opentelemetry 0.28.x | Yes -- versioned together |
| serde_yaml 0.9.x | serde 1.0+ | Yes -- standard |
| wasmtime 38.x (host) | wasm32-wasi target (guest) | Yes -- guest compiled with `rustc --target wasm32-wasi` |
### Critical Version Note
## Sources
- Context7: `/bytecodealliance/wasmtime` (v38.0.4) -- pooling allocator, ResourceLimiter, fast instantiation, security vulnerability runbook
- Context7: `/tokio-rs/axum` (v0.8.4) -- SSE streaming, WebSocket, middleware, state extraction
- Context7: `/64bit/async-openai` (v0.34.0) -- streaming chat completions, dynamic dispatch with `Box<dyn Config>`, Azure/OpenAI provider configs
- Context7: `/bigskysoftware/htmx` (v2.0.4) -- SSE extension (`sse-connect`, `sse-swap`), WebSocket support
- Context7: `/open-telemetry/opentelemetry-rust` -- OTLP exporter, Prometheus metrics, tracing layer
- Context7: `/metrics-rs/metrics` -- Counter, Gauge, Histogram handles
- Context7: `/la10736/rstest` -- async fixtures, `#[future]`, parameterized tests
- Context7: `/testcontainers/testcontainers-rs` -- async container lifecycle, Redis/PostgreSQL modules
- Context7: `/josephlenton/axum-test` -- WebSocket test assertions, SSE testing, HTTP test server
- Context7: `/websites/rs_tracing` -- structured spans, events, subscriber architecture
- Context7: `/websites/rs_tracing-subscriber` -- layered filtering, fmt layer, EnvFilter
- Context7: `/websites/rs_tracing-opentelemetry` -- OpenTelemetry layer, SdkTracerProvider
- Context7: `/websites/rs_wasmtime` (v36-38) -- WASI preview2, component model, WasiView trait
- Context7: `/websites/rs_tower-http_tower_http` -- CorsLayer, CompressionLayer, ServeDir
- Context7: `/launchbadge/sqlx` -- compile-time checked SQL, async, PostgreSQL/SQLite support
- Context7: `/websites/rs_redis` (0.32.x) -- async client, connection pooling, cluster
- Context7: `/wasmerio/wasmer` -- benchmark comparison (score 15 vs wasmtime 61.5)
- Project architecture documents: `docs/jadepaw_discussion.md` -- Wasm isolation model, instance pool design, security model
- Project constraints: `.planning/PROJECT.md` -- Rust + wasmtime + tokio, sub-5ms cold start, 10000 instances/box
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

Conventions not yet established. Will populate as patterns emerge during development.
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

Architecture not yet mapped. Follow existing patterns found in the codebase.
<!-- GSD:architecture-end -->

<!-- GSD:skills-start source:skills/ -->
## Project Skills

No project skills found. Add skills to any of: `.claude/skills/`, `.agents/skills/`, `.cursor/skills/`, `.github/skills/`, or `.codex/skills/` with a `SKILL.md` index file.
<!-- GSD:skills-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd-quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd-debug` for investigation and bug fixing
- `/gsd-execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd-profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
