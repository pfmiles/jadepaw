//! Re-export of SkillManifest from jadepaw-core.
//!
//! This module provides a crate-local re-export of `SkillManifest` so that
//! `crate::manifest::SkillManifest` resolves cleanly within jadepaw-skill.
//! The canonical definition lives in `jadepaw_core::skill_types`.

pub use jadepaw_core::SkillManifest;