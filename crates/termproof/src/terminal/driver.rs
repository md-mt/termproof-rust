//! [`SessionDriver`] — the scenario-facing way to drive a session.
//!
//! # Which one do I reach for?
//!
//! There are now two ways to talk to a terminal session, and they point in
//! opposite directions:
//!
//! - **Writing a scenario** — something that types at a program and then
//!   asserts about what it sees — use [`SessionDriver`]. It wraps a
//!   `Box<dyn Session>`, carries default timeouts, and defers errors so a
//!   failure is reported once, at the assertion, instead of at every
//!   keystroke.
//! - **Implementing a backend** — teaching TermProof to drive a new kind of
//!   terminal (a container, a remote host, a plugin) — implement
//!   [`Session`]. That trait is the backend contract and is deliberately
//!   narrow, total and un-opinionated: every call is fallible, every timeout
//!   is explicit, nothing is remembered between calls.
//!
//! `SessionDriver` is built entirely on top of the [`Session`] trait and adds
//! nothing a backend has to know about. A backend author never needs to read
//! this module; a scenario author rarely needs to read [`Session`].
//!
//! # Deferred errors
//!
//! The driver holds at most one failure: the *first* one. Once a call fails,
//! every later driving call is a no-op, and the failure is handed to you by
//! the next read or check — still naming the operation that originally failed
//! rather than whatever happened to run last.
//!
//! ```
//! use termproof::terminal::{InMemorySession, SessionDriver};
//! # use std::path::PathBuf;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # let session = InMemorySession::new(vec!["sh".into()], PathBuf::from("c.cast"), 80, 24);
//! let mut driver = SessionDriver::new(Box::new(session));
//!
//! driver.send_text("hello");
//! driver.press("enter");
//! driver.wait_for_idle();
//!
//! // If any of the three failed, this is where you hear about it.
//! driver.expect_screen_contains("hello")?;
//! # Ok(())
//! # }
//! ```
//!
//! This is not the same as swallowing the error. It is deliberately harder to
//! ignore than `let _ = ...`: the failure outlives the call that produced it,
//! and every subsequent read is fallible, so a scenario cannot reach an
//! assertion result without either surfacing the failure or clearing it on
//! purpose with [`SessionDriver::clear_failure`].

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use crate::terminal::error::SessionError;
use crate::terminal::session::Session;

/// Longest argument echoed back in an operation label before it is elided.
///
/// A label exists to identify which call failed, not to reproduce its input,
/// and a scenario that pastes a kilobyte of text should not put a kilobyte
/// into the error message.
const MAX_LABEL_ARG: usize = 48;

// ---- errors --------------------------------------------------------------

/// A failure recorded by a [`SessionDriver`].
///
/// Cloneable, because the driver hands the same failure to every read that
/// follows it rather than surrendering it to the first caller. The underlying
/// [`SessionError`] is shared rather than copied — it is not itself `Clone`.
#[derive(Debug, Clone, Error)]
pub enum DriverError {
    /// The session itself refused the operation.
    ///
    /// `op` is the label of the call that failed — `press("enter")`,
    /// `send_text("hello")` — not the call that observed the failure.
    #[error("{op} failed (driver call #{call}): {cause}")]
    Session {
        /// Label of the operation that failed, argument included.
        op: String,
        /// 1-based ordinal of that operation among this driver's calls.
        call: usize,
        /// The error the backend returned.
        cause: Arc<SessionError>,
    },

    /// The session accepted the operation but the condition never held.
    ///
    /// A wait that returns `false` is not a [`SessionError`] — nothing broke,
    /// the screen simply never did what was asked of it. The driver still
    /// treats it as a failure, because a scenario that waits for something is
    /// asserting that it happens.
    #[error("{op} was not satisfied (driver call #{call})")]
    Unsatisfied {
        /// Label of the wait that was not satisfied.
        op: String,
        /// 1-based ordinal of that operation among this driver's calls.
        call: usize,
    },

    /// An expectation about the screen did not hold.
    ///
    /// Produced by [`SessionDriver::expect_screen_contains`] and friends, not
    /// deferred: this is the assertion itself failing, with no earlier failure
    /// to blame.
    #[error("{expectation}\n--- screen ---\n{screen}\n--------------")]
    Expectation {
        /// What failed, as a complete sentence.
        ///
        /// A whole sentence rather than a noun phrase with a fixed suffix,
        /// because the suffix cannot be right for both polarities: a negative
        /// expectation fails precisely *because* the text was present.
        expectation: String,
        /// The screen at the moment the expectation was evaluated.
        screen: String,
    },
}

impl DriverError {
    /// The label of the operation this error is about.
    ///
    /// For a deferred failure this is the call that *first* went wrong, which
    /// is generally not the call the caller was making when it found out. For
    /// an [`Expectation`](Self::Expectation), which has no operation behind
    /// it, this is the expectation sentence.
    pub fn op(&self) -> &str {
        match self {
            Self::Session { op, .. } | Self::Unsatisfied { op, .. } => op,
            Self::Expectation { expectation, .. } => expectation,
        }
    }

    /// The backend error underneath, when there is one.
    pub fn session_error(&self) -> Option<&SessionError> {
        match self {
            Self::Session { cause, .. } => Some(cause),
            Self::Unsatisfied { .. } | Self::Expectation { .. } => None,
        }
    }
}

// ---- timeouts ------------------------------------------------------------

/// Default timeouts applied by a [`SessionDriver`] when a call is given none.
///
/// Each value is taken from the existing recipe path rather than invented, so
/// a scenario written against the driver waits as long as the equivalent
/// recipe would. That means matching whichever part of the recipe path
/// actually governs the wait, which is not the same source for every field —
/// see each one. They are not new policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverTimeouts {
    /// How long the screen must hold still before it counts as idle.
    ///
    /// Default: 500ms, matching the `wait_for_idle` step's `stable_seconds`.
    pub stable: Duration,

    /// How long to wait for the screen to go idle before giving up.
    ///
    /// Default: 10s, matching the `wait_for_idle` step's `timeout_seconds`.
    pub idle: Duration,

    /// How long to wait for text to appear.
    ///
    /// Default: 10s, matching the `wait_for_text` step's `timeout_seconds`.
    pub text: Duration,

    /// How long to wait for the child to exit.
    ///
    /// Default: 30s — deliberately not the 10s the other waits use. The
    /// closest recipe equivalent is the implicit exit wait `ScriptedPtyMode`
    /// performs for `expect_exit_code`, and that waits the whole recipe
    /// timeout, whose default is 30s. Matching the recipe matters more here
    /// than matching the neighbouring fields: a scenario ported from a recipe
    /// should not start timing out 20s earlier than the recipe did.
    pub exit: Duration,

    /// How long a drain of already-available output may block for.
    ///
    /// Default: 3s, the cap `ScriptedPtyMode` puts on its post-script quiesce.
    pub read: Duration,
}

impl Default for DriverTimeouts {
    fn default() -> Self {
        Self {
            stable: Duration::from_millis(500),
            idle: Duration::from_secs(10),
            text: Duration::from_secs(10),
            exit: Duration::from_secs(30),
            read: Duration::from_secs(3),
        }
    }
}

// ---- driver --------------------------------------------------------------

/// A scenario-facing wrapper around a `Box<dyn Session>`.
///
/// See the [module documentation](self) for why this exists and when to reach
/// for it instead of [`Session`].
///
/// The driving methods — [`send_text`](Self::send_text),
/// [`press`](Self::press), [`wait_for_idle`](Self::wait_for_idle) and the
/// rest — return `&mut Self` so they read as statements and chain. They never
/// return an error. The reading methods — [`screen`](Self::screen),
/// [`screen_contains`](Self::screen_contains) — and the explicit
/// [`check`](Self::check) return `Result`, and that is where a deferred
/// failure surfaces.
pub struct SessionDriver {
    session: Box<dyn Session>,
    timeouts: DriverTimeouts,
    failure: Option<DriverError>,
    /// Count of driving calls issued, whether or not they ran. Used to number
    /// the failing call in a deferred error.
    calls: usize,
}

impl std::fmt::Debug for SessionDriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn Session` is not `Debug`, so name it by its argv instead. That
        // is the identifying thing about a session anyway.
        f.debug_struct("SessionDriver")
            .field("argv", &self.session.argv())
            .field("timeouts", &self.timeouts)
            .field("failure", &self.failure)
            .field("calls", &self.calls)
            .finish()
    }
}

impl SessionDriver {
    /// Wrap a session, using [`DriverTimeouts::default`].
    pub fn new(session: Box<dyn Session>) -> Self {
        Self::with_timeouts(session, DriverTimeouts::default())
    }

    /// Wrap a session with timeouts of your own.
    pub fn with_timeouts(session: Box<dyn Session>, timeouts: DriverTimeouts) -> Self {
        Self {
            session,
            timeouts,
            failure: None,
            calls: 0,
        }
    }

    /// The timeouts this driver applies when a call is given none.
    pub fn timeouts(&self) -> &DriverTimeouts {
        &self.timeouts
    }

    /// Replace the default timeouts. Affects later calls only.
    pub fn set_timeouts(&mut self, timeouts: DriverTimeouts) -> &mut Self {
        self.timeouts = timeouts;
        self
    }

    // ---- deferred-failure state -----------------------------------------

    /// The first failure recorded, if any. Does not clear it.
    pub fn failure(&self) -> Option<&DriverError> {
        self.failure.as_ref()
    }

    /// Whether a failure is pending, and therefore whether driving calls are
    /// currently no-ops.
    pub fn is_failed(&self) -> bool {
        self.failure.is_some()
    }

    /// Surface the first failure, if there was one.
    ///
    /// The explicit form of what every read does implicitly. Put it wherever a
    /// scenario wants to stop rather than carry on into an assertion.
    pub fn check(&self) -> Result<(), DriverError> {
        match &self.failure {
            Some(err) => Err(err.clone()),
            None => Ok(()),
        }
    }

    /// Take the pending failure and re-arm the driver.
    ///
    /// For a scenario that genuinely can recover — retrying a flaky step, or
    /// probing something it expects to fail. Without this the first failure
    /// would be terminal, which is the right default and the wrong only
    /// option.
    ///
    /// The call counter is not reset: the calls that were skipped while the
    /// driver was failed still consumed their numbers, so a later failure's
    /// ordinal keeps pointing at the right place in the scenario.
    pub fn clear_failure(&mut self) -> Option<DriverError> {
        self.failure.take()
    }

    /// Record `err` if nothing has failed yet. First failure wins.
    fn fail(&mut self, err: DriverError) {
        if self.failure.is_none() {
            self.failure = Some(err);
        }
    }

    /// Run `f` unless a failure is already pending, recording any error
    /// against `op`.
    ///
    /// `op` is built by the caller *before* the call runs, so the label
    /// describes what was attempted rather than being reconstructed after the
    /// fact.
    fn drive<F>(&mut self, op: String, f: F) -> &mut Self
    where
        F: FnOnce(&mut dyn Session) -> Result<(), SessionError>,
    {
        // Counted before the early return, so the ordinal stays the position
        // of the call in the scenario. A no-op call still consumes its number.
        self.calls += 1;
        if self.failure.is_some() {
            return self;
        }
        let call = self.calls;
        if let Err(cause) = f(self.session.as_mut()) {
            self.fail(DriverError::Session {
                op,
                call,
                cause: Arc::new(cause),
            });
        }
        self
    }

    /// As [`Self::drive`], but for a call whose `false` means "the condition
    /// never held". `require` decides whether that counts as a failure.
    fn drive_wait<F>(&mut self, op: String, require: bool, f: F) -> &mut Self
    where
        F: FnOnce(&mut dyn Session) -> Result<bool, SessionError>,
    {
        // Counted before the early return — see `drive`.
        self.calls += 1;
        if self.failure.is_some() {
            return self;
        }
        let call = self.calls;
        match f(self.session.as_mut()) {
            Ok(true) => {}
            Ok(false) => {
                if require {
                    self.fail(DriverError::Unsatisfied { op, call });
                }
            }
            Err(cause) => self.fail(DriverError::Session {
                op,
                call,
                cause: Arc::new(cause),
            }),
        }
        self
    }

    // ---- driving ---------------------------------------------------------

    /// Send raw text, without a trailing newline.
    pub fn send_text(&mut self, text: &str) -> &mut Self {
        let op = label("send_text", &[quoted(text)]);
        self.drive(op, |s| s.send_text(text))
    }

    /// Send text followed by enter.
    pub fn send_line(&mut self, text: &str) -> &mut Self {
        let op = label("send_line", &[quoted(text)]);
        self.drive(op, |s| s.send_line(text))
    }

    /// Press a named key (`enter`, `escape`, `tab`, `up`, `ctrl-c`, ...).
    pub fn press(&mut self, key: &str) -> &mut Self {
        let op = label("press", &[quoted(key)]);
        self.drive(op, |s| s.press(key))
    }

    /// Wait for the screen to hold still, using the default stable window and
    /// timeout.
    ///
    /// This is the call the two-`Duration` signature was getting in the way
    /// of. A screen that never settles is a failure; use [`Self::settle`] if
    /// you want the wait to be advisory.
    pub fn wait_for_idle(&mut self) -> &mut Self {
        let (stable, timeout) = (self.timeouts.stable, self.timeouts.idle);
        self.wait_for_idle_within(stable, timeout)
    }

    /// [`Self::wait_for_idle`] with an explicit stable window and timeout.
    pub fn wait_for_idle_within(&mut self, stable: Duration, timeout: Duration) -> &mut Self {
        let op = label(
            "wait_for_idle",
            &[format!("stable={stable:?}"), format!("timeout={timeout:?}")],
        );
        self.drive_wait(op, true, |s| s.wait_for_idle(stable, timeout))
    }

    /// Give the screen a chance to settle, without insisting that it does.
    ///
    /// The same wait as [`Self::wait_for_idle`], except that a screen still
    /// changing when the timeout expires is not recorded as a failure. For
    /// scenarios that quiesce before a screenshot and genuinely do not mind
    /// whether the app is still animating. A real [`SessionError`] is still
    /// deferred.
    pub fn settle(&mut self) -> &mut Self {
        let (stable, timeout) = (self.timeouts.stable, self.timeouts.idle);
        let op = label(
            "settle",
            &[format!("stable={stable:?}"), format!("timeout={timeout:?}")],
        );
        self.drive_wait(op, false, |s| s.wait_for_idle(stable, timeout))
    }

    /// Wait for `text` to appear, using the default text timeout.
    ///
    /// Text that never appears is a failure — a scenario that waits for
    /// something is asserting that it happens.
    pub fn wait_for_text(&mut self, text: &str) -> &mut Self {
        let timeout = self.timeouts.text;
        self.wait_for_text_within(text, timeout)
    }

    /// [`Self::wait_for_text`] with an explicit timeout.
    pub fn wait_for_text_within(&mut self, text: &str, timeout: Duration) -> &mut Self {
        let op = label(
            "wait_for_text",
            &[quoted(text), format!("timeout={timeout:?}")],
        );
        self.drive_wait(op, true, |s| s.wait_for_text(text, timeout))
    }

    /// Drain whatever output is available, using the default read timeout.
    pub fn read_available(&mut self) -> &mut Self {
        let timeout = self.timeouts.read;
        self.read_available_within(timeout)
    }

    /// [`Self::read_available`] with an explicit timeout.
    pub fn read_available_within(&mut self, timeout: Duration) -> &mut Self {
        let op = label("read_available", &[format!("timeout={timeout:?}")]);
        self.drive(op, |s| s.read_available(timeout))
    }

    /// Wait for the child to exit, using the default exit timeout.
    ///
    /// A child still running when the timeout expires is a failure.
    pub fn wait_for_exit(&mut self) -> &mut Self {
        let timeout = self.timeouts.exit;
        self.wait_for_exit_within(timeout)
    }

    /// [`Self::wait_for_exit`] with an explicit timeout.
    pub fn wait_for_exit_within(&mut self, timeout: Duration) -> &mut Self {
        let op = label("wait_for_exit", &[format!("timeout={timeout:?}")]);
        self.drive_wait(op, true, |s| Ok(s.wait_for_exit(timeout)?.is_some()))
    }

    /// Close the session.
    pub fn close(&mut self) -> &mut Self {
        let op = label("close", &[]);
        self.drive(op, |s| s.close())
    }

    // ---- reading ---------------------------------------------------------

    /// The current screen text.
    ///
    /// Fallible because a pending failure means the screen is not the screen
    /// the scenario thinks it is: some keystroke on the way here never landed.
    pub fn screen(&self) -> Result<&str, DriverError> {
        self.check()?;
        Ok(self.session.screen())
    }

    /// Whether the current screen contains `needle`.
    pub fn screen_contains(&self, needle: &str) -> Result<bool, DriverError> {
        Ok(self.screen()?.contains(needle))
    }

    /// The raw byte log, decoded lossily.
    pub fn raw_output(&self) -> Result<&str, DriverError> {
        self.check()?;
        Ok(self.session.raw_output())
    }

    /// Whether the raw output contains `needle`.
    ///
    /// Sits beside [`Self::screen_contains`] for output that scrolled off the
    /// screen, or that a full-screen application overwrote.
    pub fn raw_contains(&self, needle: &str) -> Result<bool, DriverError> {
        Ok(self.raw_output()?.contains(needle))
    }

    /// The collected exit code, if the child has exited.
    pub fn exit_code(&self) -> Result<Option<i32>, DriverError> {
        self.check()?;
        Ok(self.session.exit_code())
    }

    /// Whether the child is still running.
    pub fn is_alive(&mut self) -> Result<bool, DriverError> {
        self.check()?;
        Ok(self.session.is_alive())
    }

    // ---- expectations ----------------------------------------------------

    /// Assert that the screen contains `needle`.
    ///
    /// The natural end of a scenario. Reports a pending failure first — so
    /// what you hear about is the keystroke that did not land, not the text
    /// that consequently never appeared — and otherwise fails with the screen
    /// attached.
    pub fn expect_screen_contains(&self, needle: &str) -> Result<(), DriverError> {
        let screen = self.screen()?;
        if screen.contains(needle) {
            return Ok(());
        }
        Err(DriverError::Expectation {
            expectation: format!(
                "expected the screen to contain {}, but it is absent",
                quoted(needle)
            ),
            screen: screen.to_string(),
        })
    }

    /// Assert that the screen does not contain `needle`.
    pub fn expect_screen_lacks(&self, needle: &str) -> Result<(), DriverError> {
        let screen = self.screen()?;
        if !screen.contains(needle) {
            return Ok(());
        }
        Err(DriverError::Expectation {
            expectation: format!(
                "expected the screen not to contain {}, but it is present",
                quoted(needle)
            ),
            screen: screen.to_string(),
        })
    }

    // ---- escape hatches --------------------------------------------------

    /// The wrapped session, borrowed.
    ///
    /// For the calls the driver does not wrap — [`Session::cols`],
    /// [`Session::argv`], [`Session::cast_path`]. Note that reaching through
    /// the driver bypasses the deferred failure: the session may be in a state
    /// the scenario did not intend.
    pub fn session(&self) -> &dyn Session {
        self.session.as_ref()
    }

    /// The wrapped session, mutably.
    ///
    /// The way out when the driver's opinions are the wrong ones. Errors
    /// raised through this borrow are yours to handle; the driver does not see
    /// them and will not defer them.
    pub fn session_mut(&mut self) -> &mut dyn Session {
        self.session.as_mut()
    }

    /// Unwrap the session, discarding any pending failure.
    ///
    /// Prefer [`Self::finish`], which reports it.
    pub fn into_inner(self) -> Box<dyn Session> {
        self.session
    }

    /// Surface any pending failure, then unwrap the session.
    ///
    /// The end-of-scenario counterpart to [`Self::check`], for a caller that
    /// wants the session back — to close it, or to read evidence off it.
    pub fn finish(self) -> Result<Box<dyn Session>, DriverError> {
        match self.failure {
            Some(err) => Err(err),
            None => Ok(self.session),
        }
    }
}

// ---- labels --------------------------------------------------------------

/// Quote and elide, counting characters rather than bytes so the cap cannot
/// split a multi-byte character.
///
/// Escaping is total over control characters, which matters more here than in
/// most label code: this is a terminal library, so the text being labelled is
/// routinely full of escape sequences, and the label is routinely printed
/// straight to a terminal. An unescaped `ESC` would let a failing scenario's
/// own payload repaint the report that describes it. Backslash is escaped
/// first, so `\n` typed as two characters cannot be confused with a newline.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for (i, ch) in text.chars().enumerate() {
        if i == MAX_LABEL_ARG {
            out.push('…');
            break;
        }
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            // Everything else in Unicode category Cc — ESC, BEL, NUL, DEL and
            // the C1 range — rendered as an unambiguous escape.
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:02x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `name(a, b)` — the label a deferred error reports.
fn label(name: &str, args: &[String]) -> String {
    format!("{name}({})", args.join(", "))
}
