//! Workspace linkage smoke test.
//!
//! This test imports all 6 library crates to catch forgotten `pub use` re-exports
//! that `cargo build` might miss (e.g., when a crate compiles but its public
//! API is inaccessible).

// All library crates must be importable
use jadepaw_agent as agent;
use jadepaw_bus as bus;
use jadepaw_core as core;
use jadepaw_gateway as gateway;
use jadepaw_skill as skill;
use jadepaw_wasm as wasm;
// jadepaw-server is a binary crate, cannot be imported.
// Its linkage is verified by `cargo build --workspace`.

#[test]
fn all_library_crates_importable() {
    // If this compiles, all crates are linked and their `lib.rs` files are valid.
    assert!(true);
}