//! Unified error types for jadepaw.
//!
//! All errors are represented as variants of `JadepawError` with concrete
//! type information for programmatic handling.

use std::fmt;

/// Central error type for the jadepaw platform.
///
/// Variants are designed to be specific enough for programmatic handling
/// but general enough to avoid type explosion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JadepawError {
    /// The requested capability was denied by the capability whitelist.
    ///
    /// `operation` identifies the attempted operation (e.g., "file_read").
    /// `detail` provides machine-readable context (e.g., the denied path or tool id).
    CapabilityDenied {
        operation: String,
        detail: String,
    },

    /// The Wasm guest trapped during execution.
    TrapError {
        message: String,
    },

    /// A path failed validation (traversal attempt, missing sandbox, etc.).
    PathValidationError {
        path: String,
        reason: String,
    },
}

impl JadepawError {
    /// Construct a capability denied error.
    pub fn capability_denied(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::CapabilityDenied {
            operation: operation.into(),
            detail: detail.into(),
        }
    }

    /// Construct a trap error.
    pub fn trap(message: impl Into<String>) -> Self {
        Self::TrapError {
            message: message.into(),
        }
    }

    /// Construct a path validation error.
    pub fn path_validation(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::PathValidationError {
            path: path.into(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for JadepawError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapabilityDenied {
                operation,
                detail,
            } => {
                write!(
                    f,
                    "capability denied: operation '{}' was rejected: {}",
                    operation, detail
                )
            }
            Self::TrapError { message } => write!(f, "wasm trap: {}", message),
            Self::PathValidationError { path, reason } => {
                write!(f, "path validation failed for '{}': {}", path, reason)
            }
        }
    }
}

impl std::error::Error for JadepawError {}

/// Convenience type alias that uses `JadepawError`.
pub type Result<T> = std::result::Result<T, JadepawError>;