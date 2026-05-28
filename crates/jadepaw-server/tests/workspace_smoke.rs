//! Workspace linkage smoke test.
//!
//! This test imports all 6 library crates to catch forgotten `pub use` re-exports
//! that `cargo build` might miss (e.g., when a crate compiles but its public
//! API is inaccessible).
//!
//! Lives in jadepaw-server/tests/ because the server crate depends on all 6
//! library crates, and the root workspace is a virtual manifest (tests/ at root
//! are not compiled by Cargo).

// All library crates must be importable.
// The crates are empty in Phase 1 (scaffold only), so imports have no public
// items to use. #[allow(unused_imports)] is needed because clippy's -D warnings
// would otherwise fail on dead imports.
#[allow(unused_imports)]
use jadepaw_agent as agent;
#[allow(unused_imports)]
use jadepaw_bus as bus;
#[allow(unused_imports)]
use jadepaw_core as core;
#[allow(unused_imports)]
use jadepaw_gateway as gateway;
#[allow(unused_imports)]
use jadepaw_skill as skill;
#[allow(unused_imports)]
use jadepaw_wasm as wasm;
// jadepaw-server is a binary crate, cannot be imported.
// Its linkage is verified by `cargo build --workspace`.

#[test]
fn all_library_crates_importable() {
    // If this compiles, all crates are linked and their `lib.rs` files are valid.
}
