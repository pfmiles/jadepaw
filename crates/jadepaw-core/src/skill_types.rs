//! Skill type definitions: SkillId, SkillManifest, and SkillValidationError.
//!
//! Defines the core data structures for the skill system: strongly-typed
//! UUID v7 identifiers for skills, the parsed SKILL.md manifest, and
//! structured validation errors with field-level granularity.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::Deref;
use uuid::Uuid;

/// Unique identifier for a skill.
///
/// Uses UUID v7 (time-ordered) for database index friendliness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillId(Uuid);

impl SkillId {
    /// Create a new skill identifier using UUID v7.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Deref for SkillId {
    type Target = Uuid;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Default for SkillId {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Uuid> for SkillId {
    fn from(u: Uuid) -> Self {
        Self(u)
    }
}

/// Parsed and validated manifest for a SKILL.md file.
///
/// Contains all fields from the Agent Skills open standard (`name`, `description`,
/// `metadata`) plus jadepaw-specific extensions (`tools`, `constraints`, `version`,
/// `author`). The `source_path` field is populated at parse time and is not
/// deserialized from the YAML frontmatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillManifest {
    /// Skill name — must match the parent directory name and follow kebab-case rules.
    pub name: String,

    /// Human-readable description of what the skill does (max 1024 chars).
    pub description: String,

    /// Tool names declared by this skill. Maps to ToolRegistry entries at load time.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Natural language constraints injected into the system prompt.
    #[serde(default)]
    pub constraints: Option<String>,

    /// Semantic version string for the skill.
    #[serde(default)]
    pub version: Option<String>,

    /// Attribution for the skill author.
    #[serde(default)]
    pub author: Option<String>,

    /// Arbitrary metadata passthrough (standard Agent Skills field).
    #[serde(default)]
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,

    /// The file path this manifest was loaded from.
    /// Populated at parse time, never deserialized from YAML.
    #[serde(skip)]
    pub source_path: std::path::PathBuf,
}

/// Structured validation error for SKILL.md parsing.
///
/// Each variant identifies a specific validation failure with field-level
/// context, enabling API consumers to produce user-friendly error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillValidationError {
    /// The SKILL.md file has no YAML frontmatter (no --- delimiters).
    MissingFrontmatter {
        /// The file path where the missing frontmatter was detected.
        file: String,
    },

    /// A required field is missing from the YAML frontmatter.
    MissingField {
        /// The name of the missing field.
        field: String,
    },

    /// The skill name violates the Agent Skills spec naming rules.
    InvalidName {
        /// The invalid name value.
        name: String,
        /// Human-readable description of which rule was violated.
        reason: String,
    },

    /// A field exceeds its maximum allowed length.
    FieldTooLong {
        /// The name of the field that is too long.
        field: String,
        /// The maximum allowed length.
        max: usize,
        /// The actual length provided.
        actual: usize,
    },

    /// The skill name does not match the parent directory name.
    NameDirectoryMismatch {
        /// The directory name (expected skill name).
        expected_name: String,
        /// The name value from the YAML frontmatter.
        actual_name: String,
    },

    /// A tool referenced in the skill manifest does not exist in ToolRegistry.
    ToolNotFound {
        /// The name of the tool that could not be found.
        tool_name: String,
    },

    /// The YAML frontmatter could not be parsed.
    ParseError {
        /// Human-readable error message.
        message: String,
        /// The file path where the parse error occurred.
        file: String,
        /// The line number where the error was detected, if available.
        line: Option<u32>,
    },
}

impl fmt::Display for SkillValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontmatter { file } => {
                write!(
                    f,
                    "missing YAML frontmatter: the file '{}' does not contain a --- delimited frontmatter block",
                    file
                )
            }
            Self::MissingField { field } => {
                write!(
                    f,
                    "missing required field: '{}' is required but was not found in the frontmatter",
                    field
                )
            }
            Self::InvalidName { name, reason } => {
                write!(
                    f,
                    "invalid skill name '{}': {}",
                    name, reason
                )
            }
            Self::FieldTooLong {
                field,
                max,
                actual,
            } => {
                write!(
                    f,
                    "field '{}' exceeds maximum length: {} characters allowed, but got {}",
                    field, max, actual
                )
            }
            Self::NameDirectoryMismatch {
                expected_name,
                actual_name,
            } => {
                write!(
                    f,
                    "name-directory mismatch: expected name '{}' (from directory), but got '{}' in frontmatter",
                    expected_name, actual_name
                )
            }
            Self::ToolNotFound { tool_name } => {
                write!(
                    f,
                    "tool not found: the tool '{}' declared in the skill manifest does not exist in the tool registry",
                    tool_name
                )
            }
            Self::ParseError {
                message,
                file,
                line,
            } => {
                if let Some(ln) = line {
                    write!(
                        f,
                        "YAML parse error in '{}' at line {}: {}",
                        file, ln, message
                    )
                } else {
                    write!(
                        f,
                        "YAML parse error in '{}': {}",
                        file, message
                    )
                }
            }
        }
    }
}

impl std::error::Error for SkillValidationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // SkillValidationError variants are self-contained with string
        // descriptions. No chained source errors at this level.
        None
    }
}