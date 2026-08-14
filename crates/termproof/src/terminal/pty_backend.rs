//! The PTY session backend — the default way an execution mode gets a real
//! terminal.
//!
//! `SessionBackend::create_session` is expected to hand back a session that is
//! ready to be driven, so this spawns the child before boxing it. A caller
//! that received an unspawned session would have no way to start it: `spawn`
//! is inherent to `PtySession` and not part of the `Session` surface.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::terminal::backend::SessionBackend;
use crate::terminal::error::SessionError;
use crate::terminal::pty::{PtyConfig, PtySession};
use crate::terminal::session::Session;

/// Backend that runs the command on a real pseudo-terminal.
#[derive(Debug, Clone, Default)]
pub struct PtySessionBackend;

impl PtySessionBackend {
    /// Create the backend.
    pub fn new() -> Self {
        Self
    }
}

impl SessionBackend for PtySessionBackend {
    fn create_session(
        &self,
        argv: Vec<String>,
        cast_path: PathBuf,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Box<dyn Session>, SessionError> {
        let config = PtyConfig {
            argv,
            cwd: cwd.map(PathBuf::from),
            env,
            // An empty path means "not recording" rather than a file named "".
            cast_path: (!cast_path.as_os_str().is_empty()).then_some(cast_path),
        };
        let mut session = PtySession::new(config, cols, rows)?;
        session.spawn()?;
        Ok(Box::new(session))
    }

    fn name(&self) -> &str {
        "pty"
    }
}
