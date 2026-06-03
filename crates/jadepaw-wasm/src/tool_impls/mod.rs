//! Concrete `Tool` trait implementations for jadepaw-wasm.
//!
//! Each module implements the `Tool` trait from `jadepaw-core` by wrapping
//! the existing Wasm sandbox host functions (file I/O) or a real HTTP client
//! (reqwest with SSRF protection).
//!
//! # Design (D-01a, D-03, D-04b)
//!
//! - `Tool` = agent-level dispatch (ReAct loop calls this)
//! - `HostFunctions` = Wasm-level contract (guest-host FFI)
//! - File tools reuse the Wasm sandbox path validation and capability checks
//! - HTTP tool uses reqwest with defense-in-depth SSRF (domain + IP layer)

pub mod file_tool;
pub mod http_tool;

pub use file_tool::{FileReadTool, FileWriteTool};
pub use http_tool::HttpRequestTool;