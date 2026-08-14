//! Session backend trait — how execution modes obtain sessions.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::SessionError;
use crate::session::Session;

/// Backend that creates terminal sessions.
///
/// Built-ins use this via `ExecutionContext::create_session` rather than
/// constructing `TerminalSession` directly. Custom backends (e.g. Docker) go
/// through the same public surface — see `crate::docker`.
pub trait SessionBackend: Send + Sync {
    /// Create a new session for the given command.
    fn create_session(
        &self,
        argv: Vec<String>,
        cast_path: PathBuf,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Box<dyn Session>, SessionError>;

    /// Human-readable backend name (for diagnostics).
    fn name(&self) -> &str;
}
