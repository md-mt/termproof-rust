//! Terminal error types.

use thiserror::Error;

/// Errors produced by terminal sessions and backends.
#[derive(Debug, Error)]
pub enum SessionError {
    /// The session has not been started.
    #[error("session has not started")]
    NotStarted,

    /// The underlying process or PTY reported an IO error.
    #[error("io error: {0}")]
    Io(String),

    /// The backend was misconfigured.
    #[error("configuration error: {0}")]
    Config(String),

    /// The requested operation timed out.
    #[error("timeout: {0}")]
    Timeout(String),

    /// The session is closed.
    #[error("session closed")]
    Closed,
}
