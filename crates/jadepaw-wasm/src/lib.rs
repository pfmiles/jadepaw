//! # jadepaw-wasm
//!
//! WebAssembly runtime integration: wasmtime Engine configuration, session
//! management, host function registration, and resource limiting.
//!
//! ## What lives here
//!
//! - wasmtime Engine setup, Config, and compilation cache
//! - Pre-warmed instance pool with state injection on acquire (Phase 2+)
//! - Host function definitions and linker configuration (Phase 2+)
//! - ResourceLimiter implementations for per-instance and per-tenant caps
//! - SessionState for `Store<T>` data — per-session identity and limits
//! - Epoch ticker background thread for cooperative yielding
//! - WASI context setup and preopens directory management (Phase 2+)
//! - Tool trait implementations: FileReadTool, FileWriteTool, HttpRequestTool (Phase 4+)
//!
//! ## What does NOT live here
//!
//! - Agent loop logic (see jadepaw-agent)
//! - Core data types (see jadepaw-core)
//! - HTTP gateway transport (see jadepaw-gateway)
//! - Skill compilation (see jadepaw-skill)

pub mod capability;
pub mod engine;
pub mod epoch;
pub mod host;
pub mod limits;
pub mod linker;
pub mod path;
pub mod pool;
pub mod session;
pub mod tool_impls;

pub use engine::EngineFactory;
pub use epoch::{start_epoch_ticker, EpochTickerGuard};
pub use limits::{InstanceHardLimiter, TenantQuotaLimiter};
pub use linker::{create_linker, register_host_functions};
pub use path::{normalize_path, validate_sandbox_path};
pub use pool::{InstancePool, PoolConfig, SessionHandle};
pub use session::{SessionLimits, SessionState};
pub use tool_impls::file_tool::{FileReadTool, FileWriteTool};
pub use tool_impls::http_tool::{HttpRequestTool, HTTP_REQUEST_TOOL_NAME};