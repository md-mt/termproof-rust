//! Typed internal errors for the crate root.
//!
//! These errors are converted at the crate boundary into stable diagnostics
//! (exit codes, `ValidationIssue` paths). They use `thiserror` for ergonomic
//! `Display` impls without panics on user input.

use thiserror::Error;

/// Errors that can arise while loading or validating recipes and config.
#[derive(Debug, Error)]
pub enum CoreError {
    /// I/O failure while reading a file.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// File path that failed to read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },

    /// Parse failure for JSON or YAML inputs.
    #[error("parse error at {path}: {message}")]
    Parse {
        /// File path or logical source.
        path: String,
        /// Human-readable parse message.
        message: String,
    },

    /// Validation failure with structured issues (not a single error string).
    #[error("validation failed with {count} error(s)")]
    Validation {
        /// Number of error-severity issues.
        count: usize,
    },

    /// Config value failed semantic checks (e.g. non-finite idle cap).
    #[error("invalid config value for {field}: {message}")]
    InvalidConfig {
        /// Field name.
        field: String,
        /// Reason.
        message: String,
    },
}
