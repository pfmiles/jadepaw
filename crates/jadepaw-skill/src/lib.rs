//! # jadepaw-skill
//!
//! Skill system: declarative skill format, SKILL.md parsing and validation,
//! system prompt injection, and runtime skill management.
//!
//! ## What lives here
//!
//! - SKILL.md parser (YAML frontmatter → validated SkillManifest + Markdown body)
//! - Skill name and description validation (Agent Skills open standard rules)
//! - SkillManifest type re-exports from jadepaw-core
//!
//! ## What does NOT live here
//!
//! - Wasm runtime execution (see jadepaw-wasm)
//! - Agent loop or LLM client (see jadepaw-agent)
//! - Core data types (see jadepaw-core)

pub mod manifest;
pub mod parser;
pub mod validation;

// Future modules (activated in subsequent plans):
// pub mod registry;
// pub mod manager;
// pub mod injector;
// pub mod loader;
// pub mod index;

pub use manifest::SkillManifest;
pub use parser::parse_skill_file;
pub use validation::validate_skill_name;