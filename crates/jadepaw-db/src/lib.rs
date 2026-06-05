//! # jadepaw-db
//!
//! Database persistence layer for session state. Exposes a `SessionRepository`
//! trait with SQLite backing (single-node) and migration support.
//!
//! ## What lives here
//!
//! - `SessionRepository` trait -- canonical persistence contract
//! - `SqliteSessionRepo` -- single-node SQLite implementation
//! - `SessionSnapshot`, `SessionSummary`, `SessionStatus` -- data models
//! - Embedded SQLx migrations for schema management
//!
//! ## What does NOT live here
//!
//! - Agent loop or ReAct orchestration (see jadepaw-agent)
//! - Wasm runtime or instance management (see jadepaw-wasm)
//! - HTTP/WS transport (see jadepaw-gateway)
//! - Core data types (see jadepaw-core)

pub mod migrations;
pub mod models;
pub mod repository;
pub mod sqlite_repo;

pub use models::{SessionSnapshot, SessionStatus, SessionSummary};
pub use repository::SessionRepository;