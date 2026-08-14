//! In-memory session for testing the ExecutionContext contract.

use std::path::PathBuf;
use std::time::Duration;

use crate::error::SessionError;
use crate::session::Session;

/// Minimal in-memory session that records interactions.
///
/// Used by contract tests to prove that execution modes operate only through
/// the public `Session` trait.
#[derive(Debug)]
pub struct InMemorySession {
    argv: Vec<String>,
    cast_path: PathBuf,
    cols: u16,
    rows: u16,
    screen: String,
    raw_output: String,
    exit_code: Option<i32>,
    alive: bool,
    /// Log of `send_text` / `send_line` / `press` calls for assertions.
    pub log: Vec<String>,
}

impl InMemorySession {
    /// Create a new in-memory session.
    pub fn new(argv: Vec<String>, cast_path: PathBuf, cols: u16, rows: u16) -> Self {
        Self {
            argv,
            cast_path,
            cols,
            rows,
            screen: String::new(),
            raw_output: String::new(),
            exit_code: None,
            alive: true,
            log: Vec::new(),
        }
    }

    /// Feed bytes into raw_output/screen (test helper).
    pub fn feed(&mut self, text: &str) {
        self.raw_output.push_str(text);
        self.screen.push_str(text);
    }

    /// Set the exit code.
    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = Some(code);
        self.alive = false;
    }

    /// Directly overwrite screen (test helper — replaces contents).
    pub fn set_screen(&mut self, s: impl Into<String>) {
        self.screen = s.into();
    }

    /// Directly overwrite raw buffer (test helper — replaces contents).
    pub fn set_raw(&mut self, r: impl Into<String>) {
        self.raw_output = r.into();
    }

    /// Set alive flag (test helper — simulates process exit / restart).
    pub fn set_alive(&mut self, alive: bool) {
        self.alive = alive;
    }
}

impl Session for InMemorySession {
    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        self.log.push(format!("send_text:{text}"));
        self.raw_output.push_str(text);
        self.screen.push_str(text);
        Ok(())
    }

    fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
        self.log.push(format!("send_line:{text}"));
        let line = format!("{text}\r");
        self.raw_output.push_str(&line);
        self.screen.push_str(&line);
        Ok(())
    }

    fn press(&mut self, key: &str) -> Result<(), SessionError> {
        // Validate against the frozen key contract (same surface as PtySession).
        // Keep InMemorySession usable without the real PTY but fail closed on
        // unknown keys so `press` step failures are observable in tests.
        const KEY_MAP: &[&str] = &[
            "enter",
            "escape",
            "tab",
            "backspace",
            "up",
            "down",
            "right",
            "left",
        ];
        let normalized = key.to_ascii_lowercase();
        let valid = if let Some(suffix) = normalized.strip_prefix("ctrl-") {
            let mut chars = suffix.chars();
            matches!((chars.next(), chars.next()), (Some(c), None) if c.is_ascii_alphabetic())
        } else {
            KEY_MAP.contains(&normalized.as_str())
        };
        if !valid {
            return Err(SessionError::Config(format!("unknown key: {key}")));
        }
        self.log.push(format!("press:{key}"));
        Ok(())
    }

    fn wait_for_text(&mut self, text: &str, _timeout: Duration) -> Result<bool, SessionError> {
        Ok(self.screen.contains(text) || self.raw_output.contains(text))
    }

    fn wait_for_idle(
        &mut self,
        _stable: Duration,
        _timeout: Duration,
    ) -> Result<bool, SessionError> {
        Ok(true)
    }

    fn wait_for_exit(&mut self, _timeout: Duration) -> Result<Option<i32>, SessionError> {
        Ok(self.exit_code)
    }

    fn read_available(&mut self, _timeout: Duration) -> Result<(), SessionError> {
        Ok(())
    }

    fn is_alive(&mut self) -> bool {
        self.alive
    }

    fn close(&mut self) -> Result<(), SessionError> {
        self.alive = false;
        Ok(())
    }

    fn screen(&self) -> &str {
        &self.screen
    }

    fn raw_output(&self) -> &str {
        &self.raw_output
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    fn cols(&self) -> u16 {
        self.cols
    }

    fn rows(&self) -> u16 {
        self.rows
    }

    fn argv(&self) -> &[String] {
        &self.argv
    }

    fn cast_path(&self) -> &std::path::Path {
        &self.cast_path
    }
}
