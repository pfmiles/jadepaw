//! Database migration management.
//!
//! Uses sqlx::migrate!() to embed migrations at compile time.
//! Migrations are run in `SqliteSessionRepo::new()`.
//!
//! ## Migration strategy
//!
//! Migrations live in `crates/jadepaw-db/migrations/` and follow the sqlx
//! naming convention: `YYYYMMDDHHMMSS_description.sql`. They are embedded at
//! compile time via `sqlx::migrate!("./migrations")` and run idempotently on
//! each `SqliteSessionRepo::new()` call. Applied migrations are tracked in
//! a `_sqlx_migrations` table managed by sqlx.
//!
//! This module exists as documentation of the migration strategy and as a
//! placeholder for future direct migration access (e.g., standalone CLI
//! commands for manual migration management in cluster mode).

// Migrations are embedded via sqlx::migrate!("../migrations") in sqlite_repo.rs.
// This module is intentionally minimal — the actual migration SQL lives in
// the migrations/ directory and is run by SqliteSessionRepo::new().