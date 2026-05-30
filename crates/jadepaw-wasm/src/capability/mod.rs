//! Capability enforcement — check methods on `SessionState`.
//!
//! Per D-10 / D-11 / D-12: check methods live in a dedicated `capability`
//! module. Host functions call `caller.data().can_*(...)` at entry before
//! any side effects.
//!
//! # Default deny (D-12)
//!
//! If a capability is not explicitly granted, the check method returns `false`
//! and the host function returns a `CapabilityDenied` error to the guest.
//!
//! # Pattern matching (MVP)
//!
//! - `PathPattern`: exact string match, prefix match (pattern ends with `*`),
//!   or wildcard `*` matches everything.
//! - `DomainPattern`: exact match or wildcard prefix (e.g., `*.example.com`
//!   matches `api.example.com`).

use jadepaw_core::ToolId;

use crate::session::SessionState;

impl SessionState {
    /// Check if the guest is allowed to read the given file path.
    ///
    /// Returns `true` if `path` matches any `PathPattern` in
    /// `capabilities.can_read_files`.
    pub fn can_read_file(&self, path: &str) -> bool {
        Self::matches_any_pattern(path, &self.capabilities.can_read_files)
    }

    /// Check if the guest is allowed to write the given file path.
    ///
    /// Returns `true` if `path` matches any `PathPattern` in
    /// `capabilities.can_write_files`.
    pub fn can_write_file(&self, path: &str) -> bool {
        Self::matches_any_pattern(path, &self.capabilities.can_write_files)
    }

    /// Check if the guest is allowed to call the given tool.
    ///
    /// Returns `true` if `id` is in `capabilities.can_exec_tools`.
    pub fn can_call_tool(&self, id: &ToolId) -> bool {
        self.capabilities.can_exec_tools.contains(id)
    }

    /// Check if the guest is allowed to access the given network domain.
    ///
    /// Returns `true` if `domain` matches any `DomainPattern` in
    /// `capabilities.can_network_to`.
    pub fn can_access_domain(&self, domain: &str) -> bool {
        self.capabilities
            .can_network_to
            .iter()
            .any(|pattern| Self::domain_matches(domain, &pattern.0))
    }

    // ── Internal helper: path pattern matching ──

    /// Check if a path matches any of the path patterns.
    fn matches_any_pattern(
        path: &str,
        patterns: &[jadepaw_core::PathPattern],
    ) -> bool {
        patterns.iter().any(|p| Self::path_matches(path, &p.0))
    }

    /// Simple glob-like pattern matching for paths.
    ///
    /// Supported patterns:
    /// - `"*"` — matches any path
    /// - `"prefix/*"` — matches any path starting with "prefix/"
    /// - `"exact_file.txt"` — exact match only
    fn path_matches(path: &str, pattern: &str) -> bool {
        // Wildcard matches everything
        if pattern == "*" {
            return true;
        }

        // Prefix match: pattern ends with "/*" or just matches up to the wildcard
        if let Some(prefix) = pattern.strip_suffix("/*") {
            // Must either be exactly the prefix (empty path after prefix) or
            // the prefix followed by '/' to enforce directory boundary.
            return path == prefix
                || (path.starts_with(prefix)
                    && path.as_bytes().get(prefix.len()) == Some(&b'/'));
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return path.starts_with(prefix);
        }

        // Exact match
        path == pattern
    }

    // ── Internal helper: domain pattern matching ──

    /// Simple glob-like pattern matching for domains.
    ///
    /// Supported patterns:
    /// - `"*"` — matches any domain
    /// - `"exact.domain.com"` — exact match only
    /// - `"*.example.com"` — matches any single subdomain of example.com
    fn domain_matches(domain: &str, pattern: &str) -> bool {
        // Wildcard matches everything
        if pattern == "*" {
            return true;
        }

        // Exact match
        if domain == pattern {
            return true;
        }

        // Wildcard subdomain: "*.example.com" matches "api.example.com"
        if let Some(suffix) = pattern.strip_prefix("*.") {
            return domain.ends_with(suffix)
                // Ensure the suffix actually begins at a dot position
                && domain.len() > suffix.len()
                && domain.as_bytes()[domain.len() - suffix.len() - 1] == b'.';
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn path_matches_exact() {
        assert!(SessionState::path_matches("file.txt", "file.txt"));
        assert!(!SessionState::path_matches("file.txt.bak", "file.txt"));
    }

    #[test]
    fn path_matches_wildcard() {
        assert!(SessionState::path_matches("anything", "*"));
        assert!(SessionState::path_matches("deep/nested/path", "*"));
    }

    #[test]
    fn path_matches_prefix() {
        assert!(SessionState::path_matches("data/config.json", "data/*"));
        assert!(SessionState::path_matches("data/nested/file.txt", "data/*"));
        assert!(!SessionState::path_matches("not_data/file.txt", "data/*"));
    }

    #[test]
    fn path_matches_prefix_no_slash() {
        assert!(SessionState::path_matches("data_config", "data_*"));
        assert!(!SessionState::path_matches("different", "data_*"));
    }

    #[test]
    fn domain_matches_exact() {
        assert!(SessionState::domain_matches("api.example.com", "api.example.com"));
        assert!(!SessionState::domain_matches("other.example.com", "api.example.com"));
    }

    #[test]
    fn domain_matches_wildcard() {
        assert!(SessionState::domain_matches("api.example.com", "*.example.com"));
        assert!(SessionState::domain_matches("www.example.com", "*.example.com"));
        // Wildcard should not match the root domain
        assert!(!SessionState::domain_matches("example.com", "*.example.com"));
        // Wildcard should not match multiple subdomain levels
        assert!(!SessionState::domain_matches("api.other.com", "*.example.com"));
    }

    #[test]
    fn domain_matches_bare_star() {
        // Bare "*" matches any domain (consistent with path_matches behavior)
        assert!(SessionState::domain_matches("example.com", "*"));
        assert!(SessionState::domain_matches("api.internal.corp", "*"));
        assert!(SessionState::domain_matches("localhost", "*"));
    }
}