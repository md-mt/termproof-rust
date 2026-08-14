//! `SessionDriver` — default timeouts, screen convenience, deferred errors.
//!
//! The interesting claim under test is the deferred-error one: that the first
//! failure is the one reported, that later calls stop running, and that the
//! error names the operation that originally failed rather than whatever the
//! caller was doing when it found out.

use std::path::PathBuf;
use std::time::Duration;

use termproof::terminal::{
    DriverError, DriverTimeouts, InMemorySession, Session, SessionDriver, SessionError,
};

/// A session that records what it was asked to do and fails where told.
///
/// `InMemorySession` only fails on an unknown key, which is not enough to
/// exercise deferral across the whole surface.
struct Scripted {
    inner: InMemorySession,
    /// Operation names that should return an error instead of running.
    fail_on: Vec<&'static str>,
    /// Waits that should report "condition never held" without erroring.
    unsatisfied: Vec<&'static str>,
    /// Every call that actually reached this session.
    ran: Vec<String>,
    /// Arguments the last `wait_for_idle` was given.
    last_idle: Option<(Duration, Duration)>,
    /// Timeout the last `wait_for_text` was given.
    last_text_timeout: Option<Duration>,
    /// Timeout the last `wait_for_exit` was given.
    last_exit_timeout: Option<Duration>,
}

impl Scripted {
    fn new() -> Self {
        Self {
            inner: InMemorySession::new(
                vec!["scripted".to_string()],
                PathBuf::from("scripted.cast"),
                80,
                24,
            ),
            fail_on: Vec::new(),
            unsatisfied: Vec::new(),
            ran: Vec::new(),
            last_idle: None,
            last_text_timeout: None,
            last_exit_timeout: None,
        }
    }

    fn failing(op: &'static str) -> Self {
        let mut s = Self::new();
        s.fail_on.push(op);
        s
    }

    /// Record the call, then say whether the scripted answer is an error.
    fn enter(&mut self, op: &str) -> Result<(), SessionError> {
        self.ran.push(op.to_string());
        if self.fail_on.contains(&op) {
            return Err(SessionError::Io(format!("scripted failure in {op}")));
        }
        Ok(())
    }

    fn holds(&self, op: &str) -> bool {
        !self.unsatisfied.contains(&op)
    }
}

impl Session for Scripted {
    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        self.enter("send_text")?;
        self.inner.send_text(text)
    }

    fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
        self.enter("send_line")?;
        self.inner.send_line(text)
    }

    fn press(&mut self, key: &str) -> Result<(), SessionError> {
        self.enter("press")?;
        self.inner.press(key)
    }

    fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError> {
        self.enter("wait_for_text")?;
        self.last_text_timeout = Some(timeout);
        if !self.holds("wait_for_text") {
            return Ok(false);
        }
        self.inner.wait_for_text(text, timeout)
    }

    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, SessionError> {
        self.enter("wait_for_idle")?;
        self.last_idle = Some((stable, timeout));
        Ok(self.holds("wait_for_idle"))
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError> {
        self.enter("wait_for_exit")?;
        self.last_exit_timeout = Some(timeout);
        if !self.holds("wait_for_exit") {
            return Ok(None);
        }
        self.inner.wait_for_exit(timeout)
    }

    fn read_available(&mut self, timeout: Duration) -> Result<(), SessionError> {
        self.enter("read_available")?;
        self.inner.read_available(timeout)
    }

    fn is_alive(&mut self) -> bool {
        self.inner.is_alive()
    }

    fn close(&mut self) -> Result<(), SessionError> {
        self.enter("close")?;
        self.inner.close()
    }

    fn screen(&self) -> &str {
        self.inner.screen()
    }

    fn raw_output(&self) -> &str {
        self.inner.raw_output()
    }

    fn exit_code(&self) -> Option<i32> {
        self.inner.exit_code()
    }

    fn cols(&self) -> u16 {
        self.inner.cols()
    }

    fn rows(&self) -> u16 {
        self.inner.rows()
    }

    fn argv(&self) -> &[String] {
        self.inner.argv()
    }

    fn cast_path(&self) -> &std::path::Path {
        self.inner.cast_path()
    }
}

fn driver() -> SessionDriver {
    SessionDriver::new(Box::new(Scripted::new()))
}

// `Box<dyn Session>` cannot be downcast — `Session` has no `Any` bound and
// gaining one would change the trait — so a test that needs to inspect the
// recording after handing the session over keeps its own handle on it.
mod shared {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Handle onto a `Scripted` that both the driver and the test can hold.
    #[derive(Clone)]
    pub struct Recorder(pub Arc<Mutex<Scripted>>);

    impl Recorder {
        pub fn new(inner: Scripted) -> Self {
            Self(Arc::new(Mutex::new(inner)))
        }

        /// Calls that actually reached the session.
        pub fn ran(&self) -> Vec<String> {
            self.0.lock().expect("recorder poisoned").ran.clone()
        }

        pub fn last_idle(&self) -> Option<(Duration, Duration)> {
            self.0.lock().expect("recorder poisoned").last_idle
        }

        pub fn last_text_timeout(&self) -> Option<Duration> {
            self.0.lock().expect("recorder poisoned").last_text_timeout
        }

        pub fn last_exit_timeout(&self) -> Option<Duration> {
            self.0.lock().expect("recorder poisoned").last_exit_timeout
        }
    }

    /// The `Session` the driver holds; forwards to the shared `Scripted`.
    pub struct Proxy {
        pub rec: Recorder,
        /// `screen`/`raw_output` return `&str`, so the proxy has to own a copy
        /// rather than borrow through the mutex. Refreshed after every call
        /// that could change it.
        screen: String,
        raw: String,
    }

    impl Proxy {
        pub fn pair(inner: Scripted) -> (Self, Recorder) {
            let rec = Recorder::new(inner);
            (
                Self {
                    rec: rec.clone(),
                    screen: String::new(),
                    raw: String::new(),
                },
                rec,
            )
        }

        fn sync(&mut self) {
            let guard = self.rec.0.lock().expect("recorder poisoned");
            self.screen = guard.screen().to_string();
            self.raw = guard.raw_output().to_string();
        }

        fn with<T>(&mut self, f: impl FnOnce(&mut Scripted) -> T) -> T {
            let out = {
                let mut guard = self.rec.0.lock().expect("recorder poisoned");
                f(&mut guard)
            };
            self.sync();
            out
        }
    }

    impl Session for Proxy {
        fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
            self.with(|s| s.send_text(text))
        }

        fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
            self.with(|s| s.send_line(text))
        }

        fn press(&mut self, key: &str) -> Result<(), SessionError> {
            self.with(|s| s.press(key))
        }

        fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError> {
            self.with(|s| s.wait_for_text(text, timeout))
        }

        fn wait_for_idle(
            &mut self,
            stable: Duration,
            timeout: Duration,
        ) -> Result<bool, SessionError> {
            self.with(|s| s.wait_for_idle(stable, timeout))
        }

        fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError> {
            self.with(|s| s.wait_for_exit(timeout))
        }

        fn read_available(&mut self, timeout: Duration) -> Result<(), SessionError> {
            self.with(|s| s.read_available(timeout))
        }

        fn is_alive(&mut self) -> bool {
            self.with(|s| s.is_alive())
        }

        fn close(&mut self) -> Result<(), SessionError> {
            self.with(|s| s.close())
        }

        fn screen(&self) -> &str {
            &self.screen
        }

        fn raw_output(&self) -> &str {
            &self.raw
        }

        fn exit_code(&self) -> Option<i32> {
            self.rec.0.lock().expect("recorder poisoned").exit_code()
        }

        fn cols(&self) -> u16 {
            80
        }

        fn rows(&self) -> u16 {
            24
        }

        fn argv(&self) -> &[String] {
            // Fixed for the proxy; the tests do not inspect it.
            &[]
        }

        fn cast_path(&self) -> &std::path::Path {
            std::path::Path::new("scripted.cast")
        }
    }
}

use shared::{Proxy, Recorder};

fn recorded(inner: Scripted) -> (SessionDriver, Recorder) {
    let (proxy, rec) = Proxy::pair(inner);
    (SessionDriver::new(Box::new(proxy)), rec)
}

// ---- default timeouts ----------------------------------------------------

#[test]
fn wait_for_idle_uses_the_documented_defaults() {
    let (mut d, rec) = recorded(Scripted::new());
    d.wait_for_idle();
    assert_eq!(
        rec.last_idle(),
        Some((Duration::from_millis(500), Duration::from_secs(10))),
        "defaults must match the built-in wait_for_idle step"
    );
}

#[test]
fn per_call_override_beats_the_default() {
    let (mut d, rec) = recorded(Scripted::new());
    d.wait_for_idle_within(Duration::from_secs(3), Duration::from_secs(30));
    assert_eq!(
        rec.last_idle(),
        Some((Duration::from_secs(3), Duration::from_secs(30)))
    );
}

#[test]
fn set_timeouts_changes_later_calls() {
    let (proxy, rec) = Proxy::pair(Scripted::new());
    let mut d = SessionDriver::with_timeouts(
        Box::new(proxy),
        DriverTimeouts {
            stable: Duration::from_millis(50),
            idle: Duration::from_secs(1),
            ..DriverTimeouts::default()
        },
    );
    d.wait_for_idle();
    assert_eq!(
        rec.last_idle(),
        Some((Duration::from_millis(50), Duration::from_secs(1)))
    );

    d.set_timeouts(DriverTimeouts::default());
    d.wait_for_idle();
    assert_eq!(
        rec.last_idle(),
        Some((Duration::from_millis(500), Duration::from_secs(10)))
    );
}

#[test]
fn the_exit_default_matches_the_recipe_timeout_not_the_other_waits() {
    // `ScriptedPtyMode` waits the whole recipe timeout for an exit when a
    // recipe declares `expect_exit_code`, and that default is 30s. Ten
    // seconds here would time out a ported scenario 20s early.
    assert_eq!(DriverTimeouts::default().exit, Duration::from_secs(30));
    assert_ne!(
        DriverTimeouts::default().exit,
        DriverTimeouts::default().idle,
        "deliberately not the 10s the screen waits use"
    );

    // And it is the value that actually reaches the session.
    let (mut d, rec) = recorded(Scripted::new());
    d.wait_for_exit();
    assert_eq!(rec.last_exit_timeout(), Some(Duration::from_secs(30)));
}

#[test]
fn wait_for_text_uses_the_default_text_timeout() {
    let (mut d, rec) = recorded(Scripted::new());
    d.send_text("hello").wait_for_text("hello");
    assert_eq!(rec.last_text_timeout(), Some(Duration::from_secs(10)));
}

// ---- screen convenience --------------------------------------------------

#[test]
fn screen_contains_reads_the_screen() {
    let mut d = driver();
    d.send_text("hello world");
    assert!(d.screen_contains("hello").expect("no failure"));
    assert!(!d.screen_contains("goodbye").expect("no failure"));
    assert!(d.raw_contains("world").expect("no failure"));
}

#[test]
fn expect_screen_contains_attaches_the_screen() {
    let mut d = driver();
    d.send_text("actual contents");
    let err = d
        .expect_screen_contains("missing")
        .expect_err("text is absent");
    let rendered = err.to_string();
    assert!(rendered.contains("missing"), "names what was expected");
    assert!(
        rendered.contains("actual contents"),
        "attaches the screen: {rendered}"
    );
    assert!(matches!(err, DriverError::Expectation { .. }));
}

#[test]
fn a_positive_expectation_says_the_text_is_absent() {
    let mut d = driver();
    d.send_text("actual contents");
    let rendered = d
        .expect_screen_contains("missing")
        .expect_err("text is absent")
        .to_string();
    assert!(
        rendered.contains(r#"expected the screen to contain "missing", but it is absent"#),
        "{rendered}"
    );
}

#[test]
fn a_negative_expectation_says_the_text_is_present() {
    // The failure is that "boom" *is* on screen. A message ending "but it was
    // not present" would state the opposite of what happened.
    let mut d = driver();
    d.send_text("boom");
    let rendered = d
        .expect_screen_lacks("boom")
        .expect_err("text is present")
        .to_string();
    assert!(
        rendered.contains(r#"expected the screen not to contain "boom", but it is present"#),
        "{rendered}"
    );
    assert!(
        !rendered.contains("not present"),
        "must not claim the opposite of what happened: {rendered}"
    );
}

#[test]
fn expect_screen_lacks_is_the_negative() {
    let mut d = driver();
    d.send_text("boom");
    assert!(d.expect_screen_lacks("quiet").is_ok());
    assert!(d.expect_screen_lacks("boom").is_err());
}

// ---- deferred errors -----------------------------------------------------

#[test]
fn the_first_failure_is_the_one_reported() {
    // Two calls fail. The error must name the first, not the last.
    let mut inner = Scripted::new();
    inner.fail_on.push("send_text");
    inner.fail_on.push("close");
    let (mut d, _rec) = recorded(inner);

    d.send_text("first");
    d.close();

    let err = d.check().expect_err("a failure was recorded");
    assert!(
        err.op().starts_with("send_text"),
        "reported the first failure, got {}",
        err.op()
    );
}

#[test]
fn the_reported_error_names_the_operation_and_its_argument() {
    let (mut d, _rec) = recorded(Scripted::failing("press"));
    d.send_text("hello");
    d.press("enter");
    d.send_line("more");

    let err = d.check().expect_err("press failed");
    assert_eq!(err.op(), r#"press("enter")"#);
    let rendered = err.to_string();
    assert!(rendered.contains(r#"press("enter")"#), "{rendered}");
    assert!(
        rendered.contains("scripted failure in press"),
        "keeps the backend's own words: {rendered}"
    );
    // The failing call was the second the driver issued.
    assert!(rendered.contains("#2"), "numbers the call: {rendered}");
    assert!(matches!(err, DriverError::Session { call: 2, .. }));
}

#[test]
fn later_calls_do_not_run_after_a_failure() {
    let (mut d, rec) = recorded(Scripted::failing("send_text"));
    d.send_text("boom");
    d.press("enter");
    d.send_line("more");
    d.wait_for_idle();
    d.close();

    assert_eq!(
        rec.ran(),
        vec!["send_text".to_string()],
        "nothing after the failure should reach the session"
    );
}

#[test]
fn every_read_surfaces_the_deferred_failure() {
    let (mut d, _rec) = recorded(Scripted::failing("press"));
    d.press("enter");

    assert!(d.screen().is_err());
    assert!(d.screen_contains("anything").is_err());
    assert!(d.raw_output().is_err());
    assert!(d.raw_contains("anything").is_err());
    assert!(d.exit_code().is_err());
    assert!(d.is_alive().is_err());
    assert!(d.expect_screen_contains("anything").is_err());
    assert!(d.expect_screen_lacks("anything").is_err());
    assert!(d.check().is_err());
}

#[test]
fn a_read_does_not_consume_the_failure() {
    let (mut d, _rec) = recorded(Scripted::failing("press"));
    d.press("enter");

    let first = d.screen().expect_err("failed").op().to_string();
    let second = d.screen().expect_err("still failed").op().to_string();
    assert_eq!(first, second, "the failure is reported repeatedly");
}

#[test]
fn an_expectation_reports_the_keystroke_not_the_missing_text() {
    // The whole point: the text is genuinely absent, but the reason it is
    // absent is the keystroke that never landed, and that is what you hear.
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text("hello");
    d.wait_for_idle();

    let err = d
        .expect_screen_contains("hello")
        .expect_err("nothing was typed");
    assert!(
        matches!(err, DriverError::Session { .. }),
        "blames the send, not the assertion: {err}"
    );
    assert!(err.op().starts_with("send_text"));
}

#[test]
fn the_ordinal_counts_calls_that_were_skipped() {
    // Fail on call 1, skip calls 2 and 3, clear, then fail on call 4. The
    // ordinal names a position in the scenario, so the skipped calls must
    // still consume their numbers — otherwise this reports #2.
    let mut inner = Scripted::new();
    inner.fail_on.push("send_text");
    let (mut d, _rec) = recorded(inner);

    d.send_text("one"); // #1 — fails
    d.press("enter"); // #2 — skipped
    d.press("tab"); // #3 — skipped
    assert!(matches!(
        d.clear_failure(),
        Some(DriverError::Session { call: 1, .. })
    ));

    d.send_text("four"); // #4 — fails again
    let err = d.check().expect_err("the second send_text failed too");
    assert!(
        matches!(err, DriverError::Session { call: 4, .. }),
        "skipped calls must keep their numbers, got {err}"
    );
    assert!(err.to_string().contains("#4"), "{err}");
}

#[test]
fn clear_failure_re_arms_the_driver() {
    let (mut d, rec) = recorded(Scripted::failing("press"));
    d.press("enter");
    assert!(d.is_failed());

    let taken = d.clear_failure().expect("a failure was pending");
    assert!(taken.op().starts_with("press"));
    assert!(!d.is_failed());

    d.send_text("recovered");
    assert!(d.screen_contains("recovered").expect("driver is re-armed"));
    assert!(rec.ran().contains(&"send_text".to_string()));
}

#[test]
fn finish_reports_the_failure_and_returns_the_session_otherwise() {
    let (mut d, _rec) = recorded(Scripted::failing("press"));
    d.press("enter");
    assert!(d.finish().is_err());

    let (mut ok, _rec) = recorded(Scripted::new());
    ok.send_text("fine");
    let session = ok.finish().expect("nothing failed");
    assert!(session.screen().contains("fine"));
}

#[test]
fn session_error_is_still_reachable_through_the_driver_error() {
    let (mut d, _rec) = recorded(Scripted::failing("send_line"));
    d.send_line("x");
    let err = d.check().expect_err("failed");
    assert!(matches!(
        err.session_error(),
        Some(SessionError::Io(msg)) if msg.contains("send_line")
    ));
}

// ---- unsatisfied waits ---------------------------------------------------

#[test]
fn an_unsatisfied_wait_is_a_failure() {
    let mut inner = Scripted::new();
    inner.unsatisfied.push("wait_for_idle");
    let (mut d, _rec) = recorded(inner);

    d.wait_for_idle();
    let err = d.check().expect_err("the screen never settled");
    assert!(matches!(err, DriverError::Unsatisfied { .. }));
    assert!(err.op().starts_with("wait_for_idle"));
    assert!(err.session_error().is_none(), "nothing actually broke");
}

#[test]
fn settle_tolerates_a_screen_that_never_holds_still() {
    let mut inner = Scripted::new();
    inner.unsatisfied.push("wait_for_idle");
    let (mut d, _rec) = recorded(inner);

    d.settle();
    assert!(!d.is_failed(), "settle is advisory");
}

#[test]
fn settle_still_defers_a_real_session_error() {
    let (mut d, _rec) = recorded(Scripted::failing("wait_for_idle"));
    d.settle();
    let err = d.check().expect_err("the backend errored");
    assert!(matches!(err, DriverError::Session { .. }));
    assert!(err.op().starts_with("settle"));
}

#[test]
fn an_unsatisfied_wait_for_text_is_a_failure() {
    let mut inner = Scripted::new();
    inner.unsatisfied.push("wait_for_text");
    let (mut d, _rec) = recorded(inner);

    d.wait_for_text("never appears");
    let err = d.check().expect_err("the text never appeared");
    assert!(matches!(err, DriverError::Unsatisfied { .. }));
    assert!(err.op().contains("never appears"));
}

#[test]
fn a_child_that_does_not_exit_is_a_failure() {
    // `Scripted` reports no exit code until one is set.
    let (mut d, _rec) = recorded(Scripted::new());
    d.wait_for_exit();
    let err = d.check().expect_err("no exit code was collected");
    assert!(matches!(err, DriverError::Unsatisfied { .. }));
    assert!(err.op().starts_with("wait_for_exit"));
}

// ---- labels --------------------------------------------------------------

#[test]
fn a_long_argument_is_elided_rather_than_pasted_whole() {
    let long = "x".repeat(500);
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text(&long);

    let op = d.check().expect_err("failed").op().to_string();
    assert!(op.len() < 100, "label stays short: {} chars", op.len());
    assert!(op.contains('…'), "elision is visible: {op}");
}

#[test]
fn control_characters_in_a_label_are_escaped() {
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text("a\nb\tc");

    let op = d.check().expect_err("failed").op().to_string();
    assert_eq!(op, r#"send_text("a\nb\tc")"#);
    assert!(!op.contains('\n'), "the label stays on one line");
}

#[test]
fn a_backslash_is_escaped_so_it_cannot_forge_an_escape() {
    // Two literal characters, backslash and 'n'. Unescaped, the label renders
    // as `"\n"` — indistinguishable from an actual newline.
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text(r"a\nb");

    let op = d.check().expect_err("failed").op().to_string();
    assert_eq!(op, r#"send_text("a\\nb")"#);

    // The real newline renders differently, which is the whole point.
    let (mut real, _rec) = recorded(Scripted::failing("send_text"));
    real.send_text("a\nb");
    let newline_op = real.check().expect_err("failed").op().to_string();
    assert_eq!(newline_op, r#"send_text("a\nb")"#);
    assert_ne!(op, newline_op, "the two must not render identically");
}

#[test]
fn escape_and_other_control_characters_do_not_reach_the_terminal() {
    // A terminal library's labels are full of escape sequences, and the label
    // gets printed to a terminal. A raw ESC would let the payload repaint the
    // report describing it.
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text("a\x1b[31mred\x07\0b");

    let op = d.check().expect_err("failed").op().to_string();
    assert_eq!(op, r#"send_text("a\u{1b}[31mred\u{07}\u{00}b")"#);
    assert!(!op.contains('\x1b'), "no raw ESC survives: {op:?}");
    assert!(!op.contains('\x07'), "no raw BEL survives: {op:?}");
    assert!(
        !op.chars().any(char::is_control),
        "no control character survives: {op:?}"
    );
}

#[test]
fn eliding_does_not_split_a_multibyte_character() {
    // 100 three-byte characters: a byte-counting cap would slice one in half.
    let wide = "字".repeat(100);
    let (mut d, _rec) = recorded(Scripted::failing("send_text"));
    d.send_text(&wide);

    // Reaching here at all means no panic; check the label is well-formed.
    let op = d.check().expect_err("failed").op().to_string();
    assert!(op.starts_with(r#"send_text("字"#));
    assert!(op.ends_with(r#"…")"#), "{op}");
}

// ---- escape hatches ------------------------------------------------------

#[test]
fn the_wrapped_session_is_still_reachable() {
    let mut d = driver();
    d.send_text("hi");
    assert_eq!(d.session().cols(), 80);
    assert_eq!(d.session().rows(), 24);
    assert!(d.session_mut().is_alive());
}

#[test]
fn chaining_reads_as_statements() {
    // The shape the issue asked for: keystrokes as statements, one check.
    let mut d = driver();
    d.send_text("hello").press("enter").wait_for_idle();
    d.expect_screen_contains("hello")
        .expect("typed and settled");
}
