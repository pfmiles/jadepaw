//! Skill validation functions.
//!
//! Implements the Agent Skills open standard validation rules for skill names
//! and descriptions. Each function returns `Result<(), SkillValidationError>`
//! with a specific error variant and reason string for programmatic handling.

use jadepaw_core::SkillValidationError;

/// Validate a skill name against the Agent Skills spec rules.
///
/// Rules (all must pass):
/// - 1-64 characters in length
/// - Only lowercase ASCII letters, digits, and hyphens
/// - Must not start or end with a hyphen
/// - Must not contain consecutive hyphens ("--")
///
/// These rules are intentionally strict to prevent Unicode homoglyph attacks
/// (T-06-04) and ensure cross-platform filename safety.
pub fn validate_skill_name(name: &str) -> Result<(), SkillValidationError> {
    // Rule 1: 1-64 characters
    if name.is_empty() {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must be 1-64 characters, but was empty".into(),
        });
    }
    if name.len() > 64 {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: format!(
                "name must be 1-64 characters, but was {} characters",
                name.len()
            ),
        });
    }

    // Rule 2: Only lowercase ASCII letters, digits, and hyphens
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason:
                "name may only contain lowercase letters, numbers, and hyphens"
                    .into(),
        });
    }

    // Rule 3: Must not start with a hyphen
    if name.starts_with('-') {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must not start with a hyphen".into(),
        });
    }

    // Rule 4: Must not end with a hyphen
    if name.ends_with('-') {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must not end with a hyphen".into(),
        });
    }

    // Rule 5: Must not contain consecutive hyphens
    if name.contains("--") {
        return Err(SkillValidationError::InvalidName {
            name: name.to_string(),
            reason: "name must not contain consecutive hyphens".into(),
        });
    }

    Ok(())
}

/// Validate a skill description against the Agent Skills spec length limit.
///
/// Maximum 1024 characters per the spec. Returns `FieldTooLong` on violation.
pub fn validate_skill_description(desc: &str) -> Result<(), SkillValidationError> {
    if desc.len() > 1024 {
        return Err(SkillValidationError::FieldTooLong {
            field: "description".into(),
            max: 1024,
            actual: desc.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillValidationError;

    // ─── validate_skill_name tests ───────────────────────────────────────

    #[test]
    fn valid_kebab_case_names() {
        assert!(validate_skill_name("my-skill").is_ok());
        assert!(validate_skill_name("a").is_ok());
        assert!(validate_skill_name("abc123").is_ok());
        assert!(validate_skill_name("my-cool-skill").is_ok());
    }

    #[test]
    fn reject_empty_name() {
        let err = validate_skill_name("").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("empty"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_too_long_name() {
        let long = "a".repeat(65);
        let err = validate_skill_name(&long).unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("65"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_exactly_65_chars() {
        let name = "a".repeat(65);
        assert!(validate_skill_name(&name).is_err());
    }

    #[test]
    fn accept_exactly_64_chars() {
        let name = "a".repeat(64);
        assert!(validate_skill_name(&name).is_ok());
    }

    #[test]
    fn reject_uppercase() {
        let err = validate_skill_name("MySkill").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("lowercase"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_special_chars() {
        let err = validate_skill_name("my_skill").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("hyphens"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_unicode() {
        let err = validate_skill_name("my-skill-你好").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("hyphens"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_leading_hyphen() {
        let err = validate_skill_name("-my-skill").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("start with a hyphen"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_trailing_hyphen() {
        let err = validate_skill_name("my-skill-").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("end with a hyphen"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_consecutive_hyphens() {
        let err = validate_skill_name("my--skill").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("consecutive hyphens"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    #[test]
    fn reject_only_hyphen() {
        let err = validate_skill_name("-").unwrap_err();
        match err {
            SkillValidationError::InvalidName { reason, .. } => {
                assert!(reason.contains("start with a hyphen"));
            }
            _ => panic!("expected InvalidName"),
        }
    }

    // ─── validate_skill_description tests ────────────────────────────────

    #[test]
    fn accept_valid_description() {
        assert!(validate_skill_description("A simple description").is_ok());
    }

    #[test]
    fn accept_exactly_1024_chars() {
        let desc = "a".repeat(1024);
        assert!(validate_skill_description(&desc).is_ok());
    }

    #[test]
    fn reject_over_1024_chars() {
        let desc = "a".repeat(1025);
        let err = validate_skill_description(&desc).unwrap_err();
        match err {
            SkillValidationError::FieldTooLong {
                field,
                max,
                actual,
            } => {
                assert_eq!(field, "description");
                assert_eq!(max, 1024);
                assert_eq!(actual, 1025);
            }
            _ => panic!("expected FieldTooLong"),
        }
    }
}