//! Host function implementations — the bridge between guest Wasm and host OS.
//!
//! Each host function is registered on the `Linker<SessionState>` under the
//! `"jadepaw"` namespace (D-02). Every function accesses `caller.data()` at
//! entry before any side effects (D-11).
//!
//! # Trust boundary
//!
//! Guest-provided pointers and lengths are untrusted. Bounds-checked against
//! `Memory::data_size(&caller)` before access (Pitfall 3).

pub mod filesystem;
pub mod logging;
pub mod network;

pub use filesystem::{file_read_host_fn, file_write_host_fn};
pub use logging::log_message_host_fn;
pub use network::http_request_host_fn;