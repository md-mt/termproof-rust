//! Public session interface — the only surface that step engines and
//! execution modes may use.

use std::time::Duration;

use crate::terminal::error::SessionError;

/// Public session trait.
///
/// Execution modes and step actions must depend only on this interface;
/// they must not reach into runner-private internals. This is the
/// RUST-016 contract that closes #78.
pub trait Session: Send {
    /// Send raw text without a trailing newline.
    fn send_text(&mut self, text: &str) -> Result<(), SessionError>;

    /// Send text followed by carriage return (enter).
    fn send_line(&mut self, text: &str) -> Result<(), SessionError>;

    /// Press a named key (enter, escape, tab, up, down, left, right, ctrl-*).
    fn press(&mut self, key: &str) -> Result<(), SessionError>;

    /// Wait until `text` appears in screen or raw output.
    fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError>;

    /// Wait until the screen has been stable for `stable`.
    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, SessionError>;

    /// Wait for the child process to exit.
    fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError>;

    /// Drain any available output without blocking longer than `timeout`.
    fn read_available(&mut self, timeout: Duration) -> Result<(), SessionError>;

    /// Whether the child process is still alive.
    fn is_alive(&mut self) -> bool;

    /// Close the session and collect the exit code.
    fn close(&mut self) -> Result<(), SessionError>;

    /// Current terminal screen text.
    fn screen(&self) -> &str;

    /// Raw byte log as UTF-8 lossy string.
    fn raw_output(&self) -> &str;

    /// The screen with per-cell attributes, if this backend can produce one.
    ///
    /// `None` is a legitimate answer, and the default: a backend that only has
    /// text should say so rather than fabricate colours. Callers that want
    /// colour screenshots check for `Some` and fall back to [`Session::screen`].
    ///
    /// Returns an owned screen rather than a borrow because backends differ in
    /// whether they hold one: the pty backend can build it on demand from its
    /// parser, while a capture-based backend has to shell out for it.
    fn screen_attributed(&mut self) -> Option<crate::terminal::attributed::AttributedScreen> {
        None
    }

    /// Collected exit code, if any.
    fn exit_code(&self) -> Option<i32>;

    /// Terminal columns.
    fn cols(&self) -> u16;

    /// Terminal rows.
    fn rows(&self) -> u16;

    /// The argv used to create the session (for diagnostics).
    fn argv(&self) -> &[String];

    /// The cast path (for evidence pipeline).
    fn cast_path(&self) -> &std::path::Path;
}
