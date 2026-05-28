//! # jadepaw
//!
//! Root workspace crate — exists solely to hold the central `[features]` table
//! for aggregate feature flags (single-node, cluster) that cascade to sub-crates
//! via `crate-name/feature` syntax. No business logic lives here.