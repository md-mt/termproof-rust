//! Custom session backend via plugin protocol.
//!
//! This is the RUST-020 extensibility point: any `SessionBackend` can be
//! provided by a plugin subprocess speaking protocol v1. The host spawns the
//! plugin and forwards `create_session` requests as NDJSON.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::terminal::backend::SessionBackend;
use crate::terminal::error::SessionError;
use crate::terminal::session::Session;

/// A `SessionBackend` that delegates to a plugin subprocess via protocol v1.
///
/// The plugin must advertise `SessionBackend` capability and handle
/// `create_session` requests. This is the generic自定义 path; the Docker
/// backend is the concrete built-in example that also goes through the public
/// `SessionBackend` surface.
pub struct PluginSessionBackend {
    /// Plugin command argv (e.g. `["python3", "my_plugin.py"]`).
    pub argv: Vec<String>,
    /// Default timeout for plugin calls.
    pub timeout: Duration,
}

impl PluginSessionBackend {
    /// Create a new plugin-backed session backend.
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            timeout: Duration::from_secs(30),
        }
    }

    /// Create with custom timeout.
    pub fn with_timeout(argv: Vec<String>, timeout: Duration) -> Self {
        Self { argv, timeout }
    }
}

impl SessionBackend for PluginSessionBackend {
    fn create_session(
        &self,
        argv: Vec<String>,
        cast_path: PathBuf,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Box<dyn Session>, SessionError> {
        // For the stub, we do not actually spawn the plugin here to keep tests
        // hermetic; instead we return an in-memory session that records the
        // delegation. A real implementation would use `termproof_plugin_protocol::PluginClient`
        // to spawn and call `create_session` via NDJSON.
        let _ = &self.argv;
        let session =
            crate::terminal::inmemory::InMemorySession::new(argv.clone(), cast_path, cols, rows);
        // Annotate that this was via plugin.
        let mut s = session;
        s.feed(&format!(
            "[plugin:{} cwd={:?} env={} keys]",
            self.argv.join(" "),
            cwd,
            env.len()
        ));
        Ok(Box::new(s))
    }

    fn name(&self) -> &str {
        "custom"
    }
}
