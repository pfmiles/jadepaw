//! Path validation — prevent sandbox escape via path traversal.
//!
//! Implements `normalize_path` and `validate_sandbox_path` per the canonical
//! algorithm defined in `docs/jadepaw_discussion.md` Section 3.2 and Phase 2
//! research (RESEARCH.md lines 528-553).
//!
//! # Security model (SEC-03)
//!
//! 1. `normalize_path`: strips leading `/`, removes `.` and `..` components
//! 2. `validate_sandbox_path`: normalizes → joins with sandbox_root →
//!    `Path::canonicalize` (resolves symlinks) → verifies `starts_with(sandbox_root)`
//!
//! Both steps together prevent path traversal attacks (Pitfall 3).
//! Trust boundaries: guest-provided path strings are untrusted.
//! Must be called in every host function that touches the filesystem.

use jadepaw_core::JadepawError;
use std::path::{Path, PathBuf};

/// Normalize a guest-provided path string into a relative `PathBuf`.
///
/// Algorithm:
/// 1. Strip leading `/` to make relative
/// 2. Split by `/`
/// 3. For each component: skip `""` and `"."`; for `".."`, pop the last
///    accumulated component; otherwise push the component
/// 4. Join remaining components into a relative `PathBuf`
///
/// The result is always relative (no leading `/`). If `".."` would go above
/// the root of the path, it stays as a leading `..` in the normalized form.
/// The sandbox boundary check in `validate_sandbox_path` catches these.
///
/// # Examples
///
/// ```rust
/// # use std::path::PathBuf;
/// // (internal test — see tests/path_validation.rs)
/// // assert_eq!(normalize_path("foo/bar/../baz"), PathBuf::from("foo/baz"));
/// // assert_eq!(normalize_path("foo/../../../etc/passwd"), PathBuf::from("../etc/passwd"));
/// ```
pub fn normalize_path(path: &str) -> PathBuf {
    let mut components: Vec<&str> = Vec::new();

    for component in path.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {
                // Skip empty components (from double slashes or trailing slash)
                // and current directory markers
            }
            ".." => {
                // Pop last component if present
                if !components.is_empty() {
                    components.pop();
                }
                // If empty, the ".." goes above root — don't add it.
                // validate_sandbox_path catches traversal via canonicalize+prefix.
            }
            _ => {
                components.push(component);
            }
        }
    }

    components.into_iter().collect()
}

/// Validate a guest-provided path against the sandbox root.
///
/// This is the primary path traversal defense (SEC-03, T-02-08).
///
/// # Algorithm (per `docs/jadepaw_discussion.md` Section 3.2)
///
/// 1. Call `normalize_path(guest_path)` to remove `..` and `.` components
/// 2. Join the normalized relative path with `sandbox_root`
/// 3. Call `Path::canonicalize()` to resolve any remaining symlinks
/// 4. Verify that the resolved path starts with the sandbox root
///
/// # Returns
///
/// - `Ok(PathBuf)` — the canonicalized, validated path
/// - `Err(JadepawError::PathValidationError)` — traversal attempt, missing sandbox,
///   or canonicalization failure
///
/// # Trust boundary
///
/// `guest_path` is untrusted input from Wasm guest code. Every byte is assumed
/// malicious until proven otherwise (Pitfall 3).
pub fn validate_sandbox_path(guest_path: &str, sandbox_root: &Path) -> Result<PathBuf, JadepawError> {
    // Ensure sandbox_root exists and is canonical
    let sandbox_root = sandbox_root
        .canonicalize()
        .map_err(|e| {
            JadepawError::path_validation(
                guest_path.to_string(),
                format!("sandbox root is not accessible: {}", e),
            )
        })?;

    // Step 1: Normalize guest path (removes .., ., leading /)
    let normalized = normalize_path(guest_path);

    // Step 2: Join with sandbox root
    let candidate = sandbox_root.join(&normalized);

    // Step 3: Canonicalize to resolve symlinks and final path
    let resolved = candidate.canonicalize().map_err(|e| {
        JadepawError::path_validation(
            guest_path.to_string(),
            format!("path resolution failed: {}", e),
        )
    })?;

    // Step 4: Verify containment within sandbox
    if !resolved.starts_with(&sandbox_root) {
        return Err(JadepawError::path_validation(
            guest_path.to_string(),
            format!(
                "path traversal detected: '{}' resolves outside sandbox root ('{}')",
                guest_path,
                sandbox_root.display()
            ),
        ));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn normalize_collapses_parent() {
        assert_eq!(normalize_path("foo/bar/../baz"), PathBuf::from("foo/baz"));
    }

    #[test]
    fn normalize_multiple_parent_above() {
        // "foo/../../../etc/passwd": "foo" pops, then ".." × 2 dropped (stack empty)
        assert_eq!(
            normalize_path("foo/../../../etc/passwd"),
            PathBuf::from("etc/passwd")
        );
    }

    #[test]
    fn normalize_removes_dot() {
        assert_eq!(
            normalize_path("foo/./bar/./baz"),
            PathBuf::from("foo/bar/baz")
        );
    }

    #[test]
    fn normalize_strips_leading_slash() {
        assert_eq!(normalize_path("/foo/bar"), PathBuf::from("foo/bar"));
    }

    #[test]
    fn normalize_trailing_slash() {
        assert_eq!(normalize_path("foo/bar/"), PathBuf::from("foo/bar"));
    }

    #[test]
    fn normalize_just_dot() {
        assert_eq!(normalize_path("."), PathBuf::new());
    }

    #[test]
    fn normalize_empty() {
        assert_eq!(normalize_path(""), PathBuf::new());
    }

    #[test]
    fn normalize_all_parent_traversal_returns_empty() {
        // "../../.." -> all ".." are dropped (stack empty) -> empty
        assert_eq!(normalize_path("../../.."), PathBuf::new());
    }

    #[test]
    fn normalize_double_slash() {
        assert_eq!(normalize_path("foo//bar"), PathBuf::from("foo/bar"));
    }
}