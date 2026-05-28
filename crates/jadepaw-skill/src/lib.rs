//! # jadepaw-skill
//!
//! Skill system: declarative skill format, interactive creation via LLM dialogue,
//! natural-language-to-Wasm compilation, versioning, and distribution.
//!
//! ## What lives here
//!
//! - Declarative skill format (structured Markdown/YAML skeleton)
//! - Interactive skill creation via dialogue-guided LLM
//! - Skill compiler: natural language specification -> Wasm bytecode
//! - Skill registry: catalog, versioning, git-based distribution
//! - Skill packaging and publishing pipeline
//!
//! ## What does NOT live here
//!
//! - Wasm runtime execution (see jadepaw-wasm)
//! - Agent loop or LLM client (see jadepaw-agent)
//! - Core data types (see jadepaw-core)
//! - HTTP transport (see jadepaw-gateway)