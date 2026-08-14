//! Public session interface — the only surface that step engines and
//! execution modes may use.
//!
//! **This is the backend contract.** Implement [`Session`] when you are
//! teaching TermProof to drive a new kind of terminal. It is deliberately
//! narrow and un-opinionated: every call is fallible, every timeout is
//! explicit, and nothing is remembered between calls, so a backend has as
//! little to get right as possible.
//!
//! It is not the comfortable way to *write* a scenario. For that, wrap it in
//! [`crate::terminal::SessionDriver`], which supplies default timeouts and
//! defers errors to the assertion instead of raising them at every keystroke.

use std::time::Duration;

use crate::terminal::error::SessionError;

/// Public session trait.
///
/// Execution modes and step actions must depend only on this interface;
/// they must not reach into runner-private internals. This is the
/// RUST-016 contract that closes #78.
///
/// Implement this for a backend. To drive one from a scenario, prefer
/// [`crate::terminal::SessionDriver`].
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

    /// The directory the session was launched in.
    ///
    /// A launch directory, not a live one. It is fixed when the child starts,
    /// so a program that calls `chdir` — or a shell a recipe types `cd` into —
    /// moves on and this does not follow. Resolve a path against it only for a
    /// session you know has stayed put; no backend here can tell you whether
    /// one has.
    ///
    /// It is also not "the `cwd` the recipe asked for", nor the directory the
    /// run was started from. Backends disagree about both, so each resolves
    /// where the child *went* instead of echoing the request back: the pty
    /// backend starts the child in its home directory when the recipe named
    /// none, tmux starts the pane beside the session's own artifacts, and both
    /// quietly relocate a child sent somewhere that does not exist — which is
    /// reported as `None`, not as the path it was never in. Echoing the
    /// request would make this method agree with the recipe and disagree with
    /// the child.
    ///
    /// `None` means this backend cannot say, never that there is no directory:
    /// every child process has one. It is the answer for a session that has
    /// not spawned yet, for the doubles that own no child at all
    /// ([`crate::terminal::InMemorySession`], the Docker stub), and for a
    /// backend that can see the child was moved somewhere but not where.
    ///
    /// A borrow, because this is cached launch state: a backend resolves the
    /// directory once, when the child starts, and holds it for the life of the
    /// session. Caching it is not a concession to the return type, it is the
    /// only way to be right — the inputs (our own directory, whether a
    /// configured path exists) can move afterwards, so an answer computed on
    /// demand would drift away from where the child actually is.
    ///
    /// A *live* directory is the thing that would not fit this shape: reading
    /// one means asking the OS or the multiplexer per call, and that answer is
    /// owned. It belongs in a separate method if it is ever wanted.
    fn cwd(&self) -> Option<&std::path::Path> {
        None
    }

    /// The cast path (for evidence pipeline).
    fn cast_path(&self) -> &std::path::Path;
}

#[cfg(test)]
mod tests {
    use super::Session;
    use crate::terminal::inmemory::InMemorySession;

    #[test]
    fn a_backend_that_owns_no_child_reports_no_directory() {
        // The default, reached without an override. A double with no child
        // cannot say where anything is running, and must not invent the
        // runner's own directory as if it were the child's.
        let session = InMemorySession::new(
            vec!["sh".to_string()],
            std::path::PathBuf::from("/tmp/c.cast"),
            80,
            24,
        );
        assert_eq!(session.cwd(), None);
    }
}
