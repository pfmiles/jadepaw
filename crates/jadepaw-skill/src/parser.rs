//! SKILL.md frontmatter parser.
//!
//! Parses SKILL.md files (YAML frontmatter + Markdown body) into validated
//! `SkillManifest` structs using `gray_matter` for frontmatter extraction.
//!
//! Follows the validation-before-deserialization pattern: parse into
//! gray_matter's `Pod` for field-level access, run jadepaw-specific validation,
//! then construct `SkillManifest`. This avoids opaque serde errors and enables
//! field-specific `SkillValidationError` variants.

use gray_matter::engine::YAML;
use gray_matter::Matter;
use gray_matter::Pod;
use jadepaw_core::{SkillManifest, SkillValidationError};
use std::path::Path;

use crate::validation;

/// Maximum size of a SKILL.md file in bytes (1MB).
///
/// Files larger than this are rejected before parsing to prevent YAML bomb
/// denial-of-service attacks (T-06-01).
const MAX_SKILL_FILE_SIZE: usize = 1_000_000;

/// Parse a SKILL.md file content string into a validated SkillManifest and
/// Markdown body.
///
/// # Arguments
///
/// * `content` - The full text content of the SKILL.md file
/// * `dir_name` - The name of the parent directory (must match the skill name)
/// * `file_path` - The file path for error reporting and `source_path`
///
/// # Returns
///
/// `Ok((SkillManifest, String))` on success, `Err(SkillValidationError)` with
/// a specific variant and reason on failure.
///
/// # Validation steps
///
/// 1. Check file size does not exceed `MAX_SKILL_FILE_SIZE`
/// 2. Extract YAML frontmatter and body via gray_matter
/// 3. Validate required fields exist (name, description)
/// 4. Validate skill name against Agent Skills spec rules
/// 5. Validate description length
/// 6. Validate name matches directory name
/// 7. Extract optional jadepaw extension fields
/// 8. Construct and return `SkillManifest`
pub fn parse_skill_file(
    content: &str,
    dir_name: &str,
    file_path: &Path,
) -> Result<(SkillManifest, String), SkillValidationError> {
    let file_path_str = file_path.display().to_string();

    // Security: reject files larger than 1MB to prevent YAML bomb OOM (T-06-01)
    if content.len() > MAX_SKILL_FILE_SIZE {
        return Err(SkillValidationError::ParseError {
            message: format!(
                "file size {} bytes exceeds maximum of {} bytes",
                content.len(),
                MAX_SKILL_FILE_SIZE
            ),
            file: file_path_str,
            line: None,
        });
    }

    let matter = Matter::<YAML>::new();

    // Step 1: Parse frontmatter and body
    let parsed = match matter.parse::<Pod>(content) {
        Ok(p) => p,
        Err(e) => {
            return Err(SkillValidationError::ParseError {
                message: format!("{}", e),
                file: file_path_str,
                line: None,
            });
        }
    };

    let body = parsed.content.clone();

    // Step 2: Check that frontmatter data exists
    let data = parsed.data.ok_or_else(|| SkillValidationError::MissingFrontmatter {
        file: file_path_str.clone(),
    })?;

    // Step 3: Extract and validate "name" field
    let name = extract_required_string(&data, "name", &file_path_str)?;

    // Step 4: Extract and validate "description" field
    let description = extract_required_string(&data, "description", &file_path_str)?;

    // Step 5: Validate skill name against Agent Skills spec rules
    validation::validate_skill_name(&name)?;

    // Step 6: Validate description length
    validation::validate_skill_description(&description)?;

    // Step 7: Validate name matches directory name
    if name != dir_name {
        return Err(SkillValidationError::NameDirectoryMismatch {
            expected_name: dir_name.to_string(),
            actual_name: name,
        });
    }

    // Step 8: Extract optional jadepaw extension fields
    let tools = extract_tools_array(&data);
    let constraints = extract_optional_string(&data, "constraints");
    let version = extract_optional_string(&data, "version");
    let author = extract_optional_string(&data, "author");

    // Step 9: Extract metadata passthrough
    let metadata = extract_metadata(&data);

    // Step 10: Construct SkillManifest
    let manifest = SkillManifest {
        name,
        description,
        tools,
        constraints,
        version,
        author,
        metadata,
        source_path: file_path.to_path_buf(),
    };

    Ok((manifest, body))
}

/// Extract a required string field from the Pod.
///
/// Uses `as_hashmap()` to safely access the Pod as a key-value map, then
/// looks up the requested key. Returns `MissingField` if the Pod is not a
/// hash, the key does not exist, or its value is not a string.
///
/// Note: gray_matter's `Pod::Index<&str>` panics on missing keys, so we
/// must use `as_hashmap()` for safe access.
fn extract_required_string(
    data: &Pod,
    key: &str,
    _file_path: &str,
) -> Result<String, SkillValidationError> {
    let hash = data.as_hashmap().map_err(|_| SkillValidationError::MissingField {
        field: key.to_string(),
    })?;
    let value = hash.get(key).ok_or_else(|| SkillValidationError::MissingField {
        field: key.to_string(),
    })?;
    value.as_string().map_err(|_| SkillValidationError::MissingField {
        field: key.to_string(),
    })
}

/// Extract an optional string field from the Pod.
///
/// Returns `None` if the Pod is not a hash, the key does not exist, or the
/// value is not a string.
fn extract_optional_string(data: &Pod, key: &str) -> Option<String> {
    let hash = data.as_hashmap().ok()?;
    let value = hash.get(key)?;
    value.as_string().ok()
}

/// Extract the tools array from the Pod.
///
/// Returns a `Vec<String>` of tool names. Returns an empty vec if "tools"
/// is missing, null, or not an array.
fn extract_tools_array(data: &Pod) -> Vec<String> {
    let hash = match data.as_hashmap() {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };
    let tools_pod = match hash.get("tools") {
        Some(p) => p,
        None => return Vec::new(),
    };
    match tools_pod.as_vec() {
        Ok(vec) => vec
            .iter()
            .filter_map(|item| item.as_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Extract the metadata map from the Pod.
///
/// Converts arbitrary YAML key-value pairs from the "metadata" field into
/// `serde_json::Map<String, serde_json::Value>`. Returns `None` if "metadata"
/// is missing, null, or not a hash.
fn extract_metadata(data: &Pod) -> Option<serde_json::Map<String, serde_json::Value>> {
    let hash = data.as_hashmap().ok()?;
    let metadata_pod = hash.get("metadata")?;
    match metadata_pod.as_hashmap() {
        Ok(hash) if !hash.is_empty() => {
            let mut map = serde_json::Map::new();
            for (k, v) in hash {
                let json_val = pod_to_json_value(&v);
                map.insert(k, json_val);
            }
            Some(map)
        }
        _ => None,
    }
}

/// Convert a gray_matter Pod to a serde_json::Value.
fn pod_to_json_value(pod: &Pod) -> serde_json::Value {
    match pod {
        Pod::Null => serde_json::Value::Null,
        Pod::String(s) => serde_json::Value::String(s.clone()),
        Pod::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
        Pod::Float(f) => {
            if let Some(n) = serde_json::Number::from_f64(*f) {
                serde_json::Value::Number(n)
            } else {
                serde_json::Value::String(format!("{}", f))
            }
        }
        Pod::Boolean(b) => serde_json::Value::Bool(*b),
        Pod::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(pod_to_json_value).collect())
        }
        Pod::Hash(hash) => {
            let mut map = serde_json::Map::new();
            for (k, v) in hash {
                map.insert(k.clone(), pod_to_json_value(v));
            }
            serde_json::Value::Object(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jadepaw_core::SkillValidationError;
    use std::path::PathBuf;

    fn test_path() -> PathBuf {
        PathBuf::from("/test/my-skill/SKILL.md")
    }

    // ─── Valid input tests ───────────────────────────────────────────────

    #[test]
    fn parse_minimal_valid_skill() {
        let content = "---\nname: my-skill\ndescription: Does something useful\n---\n\n# Instructions\n\nDo the thing.\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        let (manifest, body) = result.unwrap();
        assert_eq!(manifest.name, "my-skill");
        assert_eq!(manifest.description, "Does something useful");
        assert!(manifest.tools.is_empty());
        assert!(manifest.constraints.is_none());
        assert!(manifest.version.is_none());
        assert!(manifest.author.is_none());
        assert!(manifest.metadata.is_none());
        assert!(
            body.contains("# Instructions"),
            "body should contain markdown: {}",
            body
        );
        assert!(
            body.contains("Do the thing"),
            "body should contain markdown content: {}",
            body
        );
        assert_eq!(
            manifest.source_path,
            test_path(),
            "source_path should match file_path"
        );
    }

    #[test]
    fn parse_skill_with_all_extension_fields() {
        let content = "\
---
name: full-skill
description: A skill with all jadepaw extensions
tools:
  - read_file
  - write_file
constraints: Never delete files without confirmation
version: 1.2.0
author: test-author
metadata:
  tags:
    - example
    - test
  priority: high
---
# Full Skill

Detailed instructions here.
";
        let result = parse_skill_file(content, "full-skill", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        let (manifest, body) = result.unwrap();
        assert_eq!(manifest.name, "full-skill");
        assert_eq!(manifest.description, "A skill with all jadepaw extensions");
        assert_eq!(manifest.tools, vec!["read_file", "write_file"]);
        assert_eq!(
            manifest.constraints.unwrap(),
            "Never delete files without confirmation"
        );
        assert_eq!(manifest.version.unwrap(), "1.2.0");
        assert_eq!(manifest.author.unwrap(), "test-author");
        assert!(manifest.metadata.is_some());
        let meta = manifest.metadata.unwrap();
        assert!(meta.contains_key("tags"));
        assert!(meta.contains_key("priority"));
        assert!(body.contains("Detailed instructions"));
    }

    #[test]
    fn parse_skill_with_numbers_in_name() {
        let content = "---\nname: skill-v2\ndescription: Version 2\n---\nBody\n";
        let result = parse_skill_file(content, "skill-v2", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn parse_skill_with_single_char_name() {
        let content = "---\nname: a\ndescription: Minimal name\n---\nBody\n";
        let result = parse_skill_file(content, "a", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn parse_skill_with_64_char_name() {
        let name = "a".repeat(64);
        let content = format!(
            "---\nname: {}\ndescription: Max length name\n---\nBody\n",
            name
        );
        let result = parse_skill_file(&content, &name, &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn parse_skill_empty_tools_array() {
        let content = "---\nname: my-skill\ndescription: Desc\ntools: []\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        assert!(result.is_ok());
        let (manifest, _) = result.unwrap();
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn parse_skill_with_trailing_newlines() {
        let content = "---\nname: my-skill\ndescription: Desc\n---\nBody with trailing\n\n\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        assert!(result.is_ok());
        let (_, body) = result.unwrap();
        assert!(body.contains("Body with trailing"));
    }

    #[test]
    fn parse_skill_body_preserves_markdown_formatting() {
        let content = "\
---
name: my-skill
description: A skill
---
# Heading 1

Some **bold** and *italic* text.

- List item 1
- List item 2

```rust
fn main() {
    println!(\"hello\");
}
```
";
        let result = parse_skill_file(content, "my-skill", &test_path());
        assert!(result.is_ok());
        let (_, body) = result.unwrap();
        assert!(body.contains("# Heading 1"));
        assert!(body.contains("**bold**"));
        assert!(body.contains("```rust"));
        assert!(body.contains("println!"));
    }

    // ─── Missing frontmatter tests ──────────────────────────────────────

    #[test]
    fn reject_missing_frontmatter_no_delimiters() {
        let content = "This is just markdown with no frontmatter at all.";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::MissingFrontmatter { file }) => {
                assert!(file.contains("SKILL.md"), "file should be in error: {}", file);
            }
            other => panic!("expected MissingFrontmatter, got {:?}", other),
        }
    }

    #[test]
    fn reject_missing_frontmatter_empty_file() {
        let content = "";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::MissingFrontmatter { .. }) => {}
            other => panic!("expected MissingFrontmatter, got {:?}", other),
        }
    }

    // ─── Missing required field tests ───────────────────────────────────

    #[test]
    fn reject_missing_name_field() {
        let content = "---\ndescription: No name here\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::MissingField { field }) => {
                assert_eq!(field, "name");
            }
            other => panic!("expected MissingField name, got {:?}", other),
        }
    }

    #[test]
    fn reject_missing_description_field() {
        let content = "---\nname: my-skill\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::MissingField { field }) => {
                assert_eq!(field, "description");
            }
            other => panic!("expected MissingField description, got {:?}", other),
        }
    }

    #[test]
    fn reject_empty_frontmatter() {
        let content = "---\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        // Empty frontmatter means no data, treated as MissingFrontmatter
        match result {
            Err(SkillValidationError::MissingFrontmatter { .. }) => {}
            other => panic!("expected MissingFrontmatter, got {:?}", other),
        }
    }

    // ─── Name validation tests ──────────────────────────────────────────

    #[test]
    fn reject_invalid_name_uppercase() {
        let content = "---\nname: MySkill\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "MySkill", &test_path());
        match result {
            Err(SkillValidationError::InvalidName { reason, .. }) => {
                assert!(reason.contains("lowercase"));
            }
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_name_special_chars() {
        let content = "---\nname: my_skill\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "my_skill", &test_path());
        match result {
            Err(SkillValidationError::InvalidName { reason, .. }) => {
                assert!(reason.contains("hyphens"));
            }
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_name_leading_hyphen() {
        let content = "---\nname: -bad-name\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "-bad-name", &test_path());
        match result {
            Err(SkillValidationError::InvalidName { reason, .. }) => {
                assert!(reason.contains("start with a hyphen"));
            }
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_name_trailing_hyphen() {
        let content = "---\nname: bad-name-\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "bad-name-", &test_path());
        match result {
            Err(SkillValidationError::InvalidName { reason, .. }) => {
                assert!(reason.contains("end with a hyphen"));
            }
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_name_consecutive_hyphens() {
        let content = "---\nname: bad--name\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "bad--name", &test_path());
        match result {
            Err(SkillValidationError::InvalidName { reason, .. }) => {
                assert!(reason.contains("consecutive hyphens"));
            }
            other => panic!("expected InvalidName, got {:?}", other),
        }
    }

    // ─── Description validation tests ───────────────────────────────────

    #[test]
    fn reject_description_too_long() {
        let long_desc = "a".repeat(1025);
        let content = format!(
            "---\nname: my-skill\ndescription: {}\n---\nBody\n",
            long_desc
        );
        let result = parse_skill_file(&content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::FieldTooLong {
                field,
                max,
                actual,
            }) => {
                assert_eq!(field, "description");
                assert_eq!(max, 1024);
                assert_eq!(actual, 1025);
            }
            other => panic!("expected FieldTooLong, got {:?}", other),
        }
    }

    #[test]
    fn accept_description_exactly_1024_chars() {
        let desc = "a".repeat(1024);
        let content = format!(
            "---\nname: my-skill\ndescription: {}\n---\nBody\n",
            desc
        );
        let result = parse_skill_file(&content, "my-skill", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    // ─── Name-directory mismatch tests ──────────────────────────────────

    #[test]
    fn reject_name_directory_mismatch() {
        let content = "---\nname: wrong-name\ndescription: Desc\n---\nBody\n";
        let result = parse_skill_file(content, "correct-name", &test_path());
        match result {
            Err(SkillValidationError::NameDirectoryMismatch {
                expected_name,
                actual_name,
            }) => {
                assert_eq!(expected_name, "correct-name");
                assert_eq!(actual_name, "wrong-name");
            }
            other => panic!("expected NameDirectoryMismatch, got {:?}", other),
        }
    }

    // ─── Invalid YAML tests ─────────────────────────────────────────────

    #[test]
    fn reject_invalid_yaml_syntax() {
        let content = "---\nname: [unclosed\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::ParseError { file, .. }) => {
                assert!(file.contains("SKILL.md"));
            }
            other => panic!("expected ParseError, got {:?}", other),
        }
    }

    #[test]
    fn reject_invalid_yaml_tab_indentation() {
        // Tabs are not valid YAML indentation
        let content = "---\nname: my-skill\n\tdescription: tabbed\n---\nBody\n";
        let result = parse_skill_file(content, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::ParseError { .. }) => {}
            other => panic!("expected ParseError, got {:?}", other),
        }
    }

    // ─── File size limit test ───────────────────────────────────────────

    #[test]
    fn reject_file_too_large() {
        // Content just over 1MB
        let large = "x".repeat(MAX_SKILL_FILE_SIZE + 1);
        let result = parse_skill_file(&large, "my-skill", &test_path());
        match result {
            Err(SkillValidationError::ParseError { message, .. }) => {
                assert!(
                    message.contains("exceeds maximum"),
                    "message should mention size limit: {}",
                    message
                );
            }
            other => panic!("expected ParseError for size, got {:?}", other),
        }
    }

    // ─── Edge case: skills with --- in body ─────────────────────────────

    #[test]
    fn parse_skill_with_dashes_in_body() {
        let content = "\
---
name: my-skill
description: Skill with dashes
---
Some content with --- in it.

And more text.
";
        let result = parse_skill_file(content, "my-skill", &test_path());
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
        let (_, body) = result.unwrap();
        assert!(
            body.contains("---"),
            "body should contain the dashes from the body: {}",
            body
        );
    }
}