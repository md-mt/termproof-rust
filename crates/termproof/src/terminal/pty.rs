//! PTY sessions and terminal state (RUST-006).
//!
//! `PtySession` owns a child process, a `TerminalScreen`, a `CastRecorder`,
//! and an `ActivityClock`. The platform layer is `portable-pty`, so the child
//! runs on a real pseudo-terminal with a controlling tty.
//!
//! That distinction is not cosmetic. A child on a pipe fails `isatty`, so
//! programs that gate interactive behaviour on it never enter the mode under
//! test — a terminal-verification tool that drives its subject through pipes
//! is not exercising the thing it claims to verify.
//!
//! Two consequences of a real tty are visible in this API:
//!
//! - **One stream.** A terminal has a single output stream; stdout and stderr
//!   arrive interleaved on the master and cannot be told apart.
//! - **Input is echoed.** In canonical mode the tty driver echoes what is
//!   written to it, so text sent with `send_line` appears on the screen once
//!   from the echo and again from whatever the child prints.
//!
//! # Why `portable-pty` is required at 0.9 and not 0.8
//!
//! Worth knowing if you already pin `portable-pty` elsewhere, because the
//! reason is not visible from what this module imports — every name in the
//! `use` line exists in 0.8 as well.
//!
//! One thing does not: [`PtySession::exit_signal`] reads
//! `ExitStatus::signal()`, added in 0.9.0. In 0.8.1 the signal name is a
//! private field and that is the only compile error at 0.8; everything else
//! here is unchanged across the two.
//!
//! It is a choice rather than an impossibility. 0.8 still shows the name
//! through `Display` — `"Terminated by {name}"` — and reading it back out
//! works. It is not done because a `Display` format is not semver-covered
//! surface: reword it and `exit_signal` quietly starts answering `None`,
//! which is a wrong answer about how a child died and one no compiler would
//! catch. The floor buys a typed accessor instead.
//!
//! Merge dependency: RUST-004 provides the typed `Recipe` / `CommandSpec`
//! and `SessionBackend` trait. `PtyConfig` scaffolds against the expected
//! fields.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::terminal::cast::{ActivityClock, CastRecorder};
use crate::terminal::error::SessionError;
use crate::terminal::screen::TerminalScreen;
use crate::terminal::session::Session;

/// EOT — what a terminal sends for "end of input"; closing a fd is a pipe
/// concept and means nothing to a tty.
const EOT: u8 = 0x04;

/// How long `close` waits for the child to die and for the reader thread to
/// notice, before giving up and detaching rather than blocking `Drop`.
const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// Key names that `press` understands (subset of `termproof/session.py::KEYS`).
const KEY_MAP: &[(&str, &str)] = &[
    ("enter", "\r"),
    ("escape", "\x1b"),
    ("tab", "\t"),
    ("backspace", "\x7f"),
    ("up", "\x1b[A"),
    ("down", "\x1b[B"),
    ("right", "\x1b[C"),
    ("left", "\x1b[D"),
];

/// Configuration for a PTY session.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    /// Argument vector; `argv[0]` is the executable.
    pub argv: Vec<String>,
    /// Working directory.
    pub cwd: Option<PathBuf>,
    /// Environment overrides merged over the parent env.
    pub env: HashMap<String, String>,
    /// Optional cast output path.
    pub cast_path: Option<PathBuf>,
}

impl PtyConfig {
    /// Create a config that inherits the parent environment.
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            cwd: None,
            env: HashMap::new(),
            cast_path: None,
        }
    }

    /// Set working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Insert an env var.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set cast output path.
    #[must_use]
    pub fn with_cast_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.cast_path = Some(path.into());
        self
    }
}

/// Where `portable-pty` will actually start the child.
///
/// Not "the configured directory", and emphatically not "the directory we are
/// in". `CommandBuilder` keeps the configured directory only when it *is* a
/// directory, and otherwise — including when none was configured at all —
/// starts the child in its home directory. So a pty session given no `cwd`
/// does **not** inherit the runner's directory the way the process backend
/// does, and one given a path that does not exist is silently moved rather
/// than refused.
///
/// This reproduces that choice so [`Session::cwd`] can report where the child
/// really is instead of where it was asked to be. It is a deliberate mirror of
/// a dependency's internals, held in place by
/// `tests/pty_session_trait.rs`, which compares this against what a real child
/// prints — if `portable-pty` changes the rule, that test fails rather than
/// this quietly starting to lie.
///
/// The home directory is read from the environment the child will get, in the
/// same order `CommandBuilder` reads it. Its last resort is `getpwuid`, which
/// needs `unsafe`; this crate forbids that, so an environment with no `HOME`
/// at all is the one case here that answers "cannot say".
fn launch_dir(config: &PtyConfig) -> Option<PathBuf> {
    if let Some(dir) = config.cwd.as_deref().filter(|d| d.is_dir()) {
        return Some(dir.to_path_buf());
    }
    config
        .env
        .get("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

/// Typed errors for PTY sessions.
#[derive(Debug)]
pub enum PtyError {
    /// `argv` was empty.
    EmptyArgv,
    /// Child spawn failed.
    SpawnFailed(String),
    /// I/O error.
    Io(String),
    /// Session not started.
    NotStarted,
    /// Unknown key name for `press`.
    UnknownKey(String),
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyArgv => write!(f, "argv must not be empty"),
            Self::SpawnFailed(msg) => write!(f, "failed to spawn pty child: {msg}"),
            Self::Io(msg) => write!(f, "pty I/O error: {msg}"),
            Self::NotStarted => write!(f, "pty session has not been started"),
            Self::UnknownKey(k) => write!(f, "unknown key: {k}"),
        }
    }
}

impl std::error::Error for PtyError {}

/// PTY-backed terminal session.
///
/// Spawns the child onto a pseudo-terminal, drains the master on a background
/// thread into `TerminalScreen` and `CastRecorder`, and exposes
/// `wait_for_text` / `wait_for_idle` / `resize`. `Drop` terminates the child.
///
/// It also implements [`Session`], which is how execution modes reach it.
/// Several inherent methods share a name with a trait method and differ in
/// return type — inherent `screen` yields an owned `String` read live from the
/// screen mutex, the trait's yields a `&str` borrowed from a snapshot taken at
/// the end of the last `&mut self` trait call. Rust prefers the inherent
/// method on a concrete `PtySession`, so callers that want the trait's
/// behaviour must go through `&mut dyn Session` or `Session::screen(&session)`.
pub struct PtySession {
    config: PtyConfig,
    cols: u16,
    rows: u16,
    raw_output: Arc<Mutex<Vec<u8>>>,
    screen: Arc<Mutex<TerminalScreen>>,
    activity: Arc<Mutex<ActivityClock>>,
    cast: Option<Arc<Mutex<CastRecorder>>>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    reader_handle: Option<JoinHandle<()>>,
    exit_code: Option<i32>,
    exit_signal: Option<String>,
    /// Screen text as of the last `Session` call that could have advanced it.
    ///
    /// `Session::screen` hands out a `&str` from `&self`, and the live screen
    /// lives behind a mutex shared with the reader thread, so there is no
    /// borrow to hand out. Every `&mut self` method of the trait refreshes
    /// this before returning; the inherent `screen_text` stays live.
    screen_snapshot: String,
    /// Raw output as of the last `Session` call that could have advanced it.
    raw_snapshot: String,
    /// `config.cast_path` flattened to a path so `Session::cast_path` can
    /// borrow one. Empty when the session is not recording.
    cast_path_snapshot: PathBuf,
    /// Where the child was started, resolved at spawn by [`launch_dir`].
    /// `None` until it has spawned, and after that only when the directory
    /// could not be worked out.
    launched_in: Option<PathBuf>,
}

impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession")
            .field("argv", &self.config.argv)
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("exit_code", &self.exit_code)
            .finish()
    }
}

impl PtySession {
    /// Create a session with dimensions `cols` x `rows`.
    pub fn new(config: PtyConfig, cols: u16, rows: u16) -> Result<Self, PtyError> {
        if config.argv.is_empty() {
            return Err(PtyError::EmptyArgv);
        }
        if cols == 0 || rows == 0 {
            return Err(PtyError::SpawnFailed("cols and rows must be > 0".into()));
        }
        let screen = TerminalScreen::new(cols, rows);
        let cast_path_snapshot = config.cast_path.clone().unwrap_or_default();
        Ok(Self {
            config,
            cols,
            rows,
            raw_output: Arc::new(Mutex::new(Vec::new())),
            screen: Arc::new(Mutex::new(screen)),
            activity: Arc::new(Mutex::new(ActivityClock::new())),
            cast: None,
            master: None,
            writer: None,
            child: None,
            reader_handle: None,
            exit_code: None,
            exit_signal: None,
            screen_snapshot: String::new(),
            raw_snapshot: String::new(),
            cast_path_snapshot,
            launched_in: None,
        })
    }

    /// Convenience: argv/cwd/env without cast.
    pub fn from_parts(
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Self, PtyError> {
        Self::new(
            PtyConfig {
                argv,
                cwd,
                env,
                cast_path: None,
            },
            cols,
            rows,
        )
    }

    /// Spawn the child.
    pub fn spawn(&mut self) -> Result<(), PtyError> {
        if self.child.is_some() {
            return Ok(());
        }
        if let Some(path) = self.config.cast_path.clone() {
            if let Ok(rec) = CastRecorder::new(path, self.cols, self.rows, self.config.argv.clone())
            {
                self.cast = Some(Arc::new(Mutex::new(rec)));
            }
        }

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: self.rows,
                cols: self.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::SpawnFailed(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&self.config.argv[0]);
        cmd.args(&self.config.argv[1..]);
        if let Some(cwd) = &self.config.cwd {
            cmd.cwd(cwd);
        }
        for (k, v) in &self.config.env {
            cmd.env(k, v);
        }
        if std::env::var("TERM").unwrap_or_default().is_empty()
            && !self.config.env.contains_key("TERM")
        {
            cmd.env("TERM", "xterm-256color");
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnFailed(e.to_string()))?;
        // Drop our slave handle immediately. While this process still holds a
        // slave fd open, reads on the master never see EOF, so the reader
        // thread would outlive the child and `close` would block forever.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Io(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Io(e.to_string()))?;

        let raw_out = Arc::clone(&self.raw_output);
        let screen_out = Arc::clone(&self.screen);
        let activity_out = Arc::clone(&self.activity);
        let cast_out = self.cast.clone();
        let reader_handle = thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                // A pty master reports the child's exit as EIO rather than a
                // clean EOF, so any error ends the drain just as `Ok(0)` does.
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        if let Ok(mut r) = raw_out.lock() {
                            r.extend_from_slice(chunk);
                        }
                        if let Ok(mut s) = screen_out.lock() {
                            s.feed_bytes(chunk);
                        }
                        if let Ok(mut a) = activity_out.lock() {
                            a.mark();
                        }
                        if let Some(c) = cast_out.as_ref() {
                            if let Ok(mut rec) = c.lock() {
                                rec.record_output(&String::from_utf8_lossy(chunk));
                            }
                        }
                    }
                }
            }
        });

        self.writer = Some(writer);
        self.master = Some(pair.master);
        self.child = Some(child);
        self.reader_handle = Some(reader_handle);
        // Resolved once the child exists, so "has spawned" and "has a launch
        // directory" are the same condition, and never from state that has had
        // time to move since.
        self.launched_in = launch_dir(&self.config);
        Ok(())
    }

    /// Current raw output bytes (cloned).
    pub fn raw_output(&self) -> Vec<u8> {
        self.raw_output
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Number of raw output bytes seen so far, without copying them.
    pub fn raw_output_len(&self) -> usize {
        self.raw_output.lock().map(|r| r.len()).unwrap_or_default()
    }

    /// Current raw output as lossy UTF-8.
    pub fn raw_output_str(&self) -> String {
        String::from_utf8_lossy(&self.raw_output()).to_string()
    }

    /// Current screen text.
    pub fn screen_text(&self) -> String {
        self.screen.lock().map(|s| s.contents()).unwrap_or_default()
    }

    /// Alias for `screen_text`.
    pub fn screen(&self) -> String {
        self.screen_text()
    }

    /// Exit code if reaped.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Name of the signal that killed the child, if it was signalled.
    ///
    /// `portable-pty` surfaces the signal by name rather than by number, so
    /// this is reported separately instead of being folded into `exit_code`.
    /// The name is the platform's — `strsignal`, so `"Hangup"` on Linux and
    /// `"Hangup: 1"` on macOS — which is why nothing here matches on it.
    ///
    /// This is the whole reason the workspace floors `portable-pty` at 0.9;
    /// see the module docs. It is not on [`Session`], so a caller reaches it
    /// only through a concrete `PtySession`.
    pub fn exit_signal(&self) -> Option<&str> {
        self.exit_signal.as_deref()
    }

    /// Process id of the child, if it has been spawned.
    pub fn process_id(&self) -> Option<u32> {
        self.child.as_ref().and_then(|c| c.process_id())
    }

    /// Whether the child is still alive.
    pub fn is_alive(&mut self) -> bool {
        let status = match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => return false,
        };
        match status {
            Ok(None) => true,
            Ok(Some(status)) => {
                if self.exit_code.is_none() {
                    self.exit_code = Some(i32::try_from(status.exit_code()).unwrap_or(-1));
                    self.exit_signal = status.signal().map(str::to_string);
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Write bytes to the terminal.
    pub fn send_bytes(&mut self, data: &[u8]) -> Result<(), PtyError> {
        let writer = self.writer.as_mut().ok_or(PtyError::NotStarted)?;
        writer
            .write_all(data)
            .map_err(|e| PtyError::Io(e.to_string()))?;
        writer.flush().map_err(|e| PtyError::Io(e.to_string()))?;
        if let Some(c) = self.cast.as_ref() {
            if let Ok(mut rec) = c.lock() {
                rec.record_input(&String::from_utf8_lossy(data));
            }
        }
        Ok(())
    }

    /// Send text without newline.
    pub fn send_text(&mut self, text: &str) -> Result<(), PtyError> {
        self.send_bytes(text.as_bytes())
    }

    /// Send a line (text + `\r`).
    pub fn send_line(&mut self, text: &str) -> Result<(), PtyError> {
        let mut buf = text.as_bytes().to_vec();
        buf.push(b'\r');
        self.send_bytes(&buf)
    }

    /// Press a named key.
    pub fn press(&mut self, key: &str) -> Result<(), PtyError> {
        let normalized = key.to_ascii_lowercase();
        if let Some(stripped) = normalized.strip_prefix("ctrl-") {
            if let Some(ch) = stripped.chars().next() {
                let lower = ch.to_ascii_lowercase() as u8;
                if lower.is_ascii_lowercase() {
                    let ctrl = lower - b'a' + 1;
                    return self.send_bytes(&[ctrl]);
                }
                return Err(PtyError::UnknownKey(key.to_string()));
            }
            return Err(PtyError::UnknownKey(key.to_string()));
        }
        if let Some((_, seq)) = KEY_MAP.iter().find(|(k, _)| *k == normalized) {
            return self.send_bytes(seq.as_bytes());
        }
        Err(PtyError::UnknownKey(key.to_string()))
    }

    /// Send EOF.
    ///
    /// On a tty this is the EOT character, not a closed file descriptor: the
    /// line discipline turns EOT into an end-of-input for the reader. The
    /// writer is then dropped, so no further input can be sent.
    pub fn send_eof(&mut self) -> Result<(), PtyError> {
        if self.writer.is_some() {
            self.send_bytes(&[EOT])?;
            self.writer.take();
        }
        Ok(())
    }

    /// Set echo.
    ///
    /// Not supported: `portable-pty` 0.9 can read the master's termios but
    /// offers no way to set it, so the tty stays in its default canonical,
    /// echoing mode. Kept as a no-op for API symmetry.
    pub fn set_echo(&mut self, _enabled: bool) -> Result<(), PtyError> {
        Ok(())
    }

    /// Resize the PTY and screen.
    ///
    /// This updates the kernel's winsize for the tty, which is what makes the
    /// child see the new dimensions (and receive `SIGWINCH`); the local screen
    /// is resized to match.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
        if cols == 0 || rows == 0 {
            return Err(PtyError::SpawnFailed("cols and rows must be > 0".into()));
        }
        self.cols = cols;
        self.rows = rows;
        if let Some(master) = self.master.as_ref() {
            master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| PtyError::Io(e.to_string()))?;
        }
        if let Ok(mut s) = self.screen.lock() {
            s.resize(cols, rows);
        }
        Ok(())
    }

    /// Poll for output (sleep to let reader thread drain).
    pub fn read_available(&mut self, timeout: Duration) {
        if timeout.is_zero() {
            thread::sleep(Duration::from_millis(5));
        } else {
            thread::sleep(timeout);
        }
    }

    /// Wait until `text` appears in screen or raw output.
    pub fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, PtyError> {
        if self.child.is_none() {
            return Err(PtyError::NotStarted);
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.read_available(Duration::from_millis(50));
            let screen = self.screen_text();
            let raw = self.raw_output_str();
            if screen.contains(text) || raw.contains(text) {
                return Ok(true);
            }
            if !self.is_alive() {
                self.read_available(Duration::from_millis(20));
                let screen = self.screen_text();
                let raw = self.raw_output_str();
                return Ok(screen.contains(text) || raw.contains(text));
            }
        }
        Ok(false)
    }

    /// Wait until screen *and* raw output have been stable for `stable`
    /// within `timeout`.
    ///
    /// The raw length matters: a child repainting the same cells leaves the
    /// screen byte-identical while still producing output, and calling that
    /// idle settles on a session that is mid-draw.
    pub fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, PtyError> {
        if self.child.is_none() {
            return Err(PtyError::NotStarted);
        }
        let deadline = Instant::now() + timeout;
        let mut last_screen = self.screen_text();
        let mut last_raw_len = self.raw_output_len();
        let mut stable_since = Instant::now();
        while Instant::now() < deadline {
            self.read_available(Duration::from_millis(50));
            let current = self.screen_text();
            let current_raw_len = self.raw_output_len();
            if current != last_screen || current_raw_len != last_raw_len {
                last_screen = current;
                last_raw_len = current_raw_len;
                stable_since = Instant::now();
            }
            if stable_since.elapsed() >= stable {
                return Ok(true);
            }
            if !self.is_alive() {
                self.read_available(Duration::from_millis(20));
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Wait for the child to exit, up to `timeout`.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, PtyError> {
        if self.child.is_none() {
            return Err(PtyError::NotStarted);
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.read_available(Duration::from_millis(50));
            if !self.is_alive() {
                return Ok(self.exit_code);
            }
        }
        Ok(self.exit_code)
    }

    /// Terminate the child.
    pub fn terminate(&mut self) -> Result<(), PtyError> {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        Ok(())
    }

    /// Close the session, terminating the child if needed and joining the
    /// reader thread.
    ///
    /// This runs from `Drop`, so it must not be able to block indefinitely.
    /// A child that has forked a grandchild holding the slave open keeps the
    /// master readable after the child itself is gone; rather than wait for a
    /// read that may never return, the reader thread is detached once
    /// `CLOSE_GRACE` elapses.
    pub fn close(&mut self) {
        if self.is_alive() {
            let _ = self.terminate();
        }
        let deadline = Instant::now() + CLOSE_GRACE;
        while self.is_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        // Release our handles so the reader's blocking read can unblock.
        self.writer.take();
        if let Some(handle) = self.reader_handle.take() {
            let join_deadline = Instant::now() + CLOSE_GRACE;
            while !handle.is_finished() && Instant::now() < join_deadline {
                thread::sleep(Duration::from_millis(5));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
            // Otherwise leave it detached rather than hang `Drop`.
        }
        self.master.take();
        self.child.take();

        if let Some(c) = self.cast.take() {
            if let Ok(mut rec) = c.lock() {
                let _ = rec.finish();
            }
        }
    }

    /// Refresh the snapshots the `Session` accessors borrow from.
    fn sync_snapshot(&mut self) {
        self.screen_snapshot = self.screen_text();
        self.raw_snapshot = self.raw_output_str();
    }

    /// Dimensions.
    pub fn dimensions(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// Columns.
    pub fn cols(&self) -> u16 {
        self.cols
    }

    /// Rows.
    pub fn rows(&self) -> u16 {
        self.rows
    }
}

impl From<PtyError> for SessionError {
    fn from(err: PtyError) -> Self {
        match err {
            // Recipe-authored input the session cannot honour: an empty argv,
            // dimensions the kernel will not take, a key that is not in the
            // map. These are configuration faults, not I/O faults.
            PtyError::EmptyArgv | PtyError::SpawnFailed(_) | PtyError::UnknownKey(_) => {
                SessionError::Config(err.to_string())
            }
            PtyError::Io(msg) => SessionError::Io(msg),
            PtyError::NotStarted => SessionError::NotStarted,
        }
    }
}

impl Session for PtySession {
    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        PtySession::send_text(self, text)?;
        self.sync_snapshot();
        Ok(())
    }

    fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
        PtySession::send_line(self, text)?;
        self.sync_snapshot();
        Ok(())
    }

    fn press(&mut self, key: &str) -> Result<(), SessionError> {
        PtySession::press(self, key)?;
        self.sync_snapshot();
        Ok(())
    }

    fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError> {
        let found = PtySession::wait_for_text(self, text, timeout)?;
        self.sync_snapshot();
        Ok(found)
    }

    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, SessionError> {
        let idle = PtySession::wait_for_idle(self, stable, timeout)?;
        self.sync_snapshot();
        Ok(idle)
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError> {
        let code = PtySession::wait_for_exit(self, timeout)?;
        self.sync_snapshot();
        Ok(code)
    }

    fn read_available(&mut self, timeout: Duration) -> Result<(), SessionError> {
        PtySession::read_available(self, timeout);
        self.sync_snapshot();
        Ok(())
    }

    fn is_alive(&mut self) -> bool {
        PtySession::is_alive(self)
    }

    fn close(&mut self) -> Result<(), SessionError> {
        PtySession::close(self);
        self.sync_snapshot();
        Ok(())
    }

    fn screen(&self) -> &str {
        &self.screen_snapshot
    }

    fn screen_attributed(&mut self) -> Option<crate::terminal::attributed::AttributedScreen> {
        // Read live from the screen mutex rather than the snapshot `screen()`
        // hands out: an attributed grid is only useful if it matches now.
        self.screen.lock().ok().map(|s| s.attributed())
    }

    fn raw_output(&self) -> &str {
        &self.raw_snapshot
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
        &self.config.argv
    }

    fn cwd(&self) -> Option<&std::path::Path> {
        // Where the child started. A pty carries no channel for the child to
        // report a later `chdir` on, and reading it out of the OS is not
        // portable, so the launch directory is the whole of what this backend
        // can honestly claim.
        self.launched_in.as_deref()
    }

    fn cast_path(&self) -> &std::path::Path {
        &self.cast_path_snapshot
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn spawns_and_captures_text() {
        let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "echo hello".into()]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let found = sess
            .wait_for_text("hello", Duration::from_secs(2))
            .expect("wait");
        assert!(found, "hello not found, screen={:?}", sess.screen_text());
        sess.close();
        assert!(sess.exit_code().is_some());
    }

    #[test]
    fn unicode_is_preserved() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "printf 'héllo 🌍\\n'".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let found = sess
            .wait_for_text("héllo", Duration::from_secs(2))
            .expect("wait");
        assert!(found, "unicode not found: {:?}", sess.screen_text());
        sess.close();
    }

    #[test]
    fn resize_changes_dimensions() {
        let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "sleep 0.5".into()]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        sess.resize(40, 10).expect("resize");
        assert_eq!(sess.dimensions(), (40, 10));
        sess.resize(100, 30).expect("resize2");
        assert_eq!(sess.dimensions(), (100, 30));
        sess.close();
    }

    #[test]
    fn send_text_and_wait() {
        let config = PtyConfig::new(vec!["cat".into()]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        thread::sleep(Duration::from_millis(100));
        sess.send_line("hello pty").expect("send");
        let found = sess
            .wait_for_text("hello pty", Duration::from_secs(2))
            .expect("wait");
        assert!(found, "echo not found: {:?}", sess.screen_text());
        sess.close();
    }

    #[test]
    fn wait_for_idle_returns_on_stable_screen() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "echo hi; sleep 0.1; echo done".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let idle = sess
            .wait_for_idle(Duration::from_millis(150), Duration::from_secs(2))
            .expect("idle");
        assert!(idle);
        sess.close();
    }

    #[test]
    fn press_unknown_key_is_error() {
        let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "sleep 0.1".into()]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let err = sess.press("f13").unwrap_err();
        assert!(matches!(err, PtyError::UnknownKey(_)));
        sess.close();
    }

    /// A child on a pipe has no controlling tty, so `test -t` fails and
    /// anything that gates interactive behaviour on `isatty` never runs.
    #[test]
    fn child_gets_a_controlling_tty() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "test -t 0 && test -t 1 && echo HAS_TTY || echo NO_TTY".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let found = sess
            .wait_for_text("HAS_TTY", Duration::from_secs(5))
            .expect("wait");
        assert!(found, "child had no tty: {:?}", sess.raw_output_str());
        sess.close();
    }

    /// The kernel must learn the new window size, not just our bookkeeping:
    /// `stty size` reads it back from the tty itself.
    #[test]
    fn resize_reaches_the_kernel() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "sleep 0.4; stty size".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        sess.resize(40, 10).expect("resize");
        let found = sess
            .wait_for_text("10 40", Duration::from_secs(5))
            .expect("wait");
        assert!(
            found,
            "child did not see the new size: {:?}",
            sess.raw_output_str()
        );
        sess.close();
    }

    /// A child can be busy without changing the screen: repainting the same
    /// cells leaves `screen_text` byte-identical while raw output keeps
    /// growing. Idle must mean "screen *and* raw output stopped", or a
    /// repainting TUI is declared settled while it is still working.
    ///
    /// This only became reachable once the screen was a real emulator. The
    /// strip-escapes stub was append-only, so any output at all moved the
    /// screen and screen-only idle detection was accidentally sufficient.
    #[test]
    fn idle_accounts_for_output_that_does_not_change_the_screen() {
        // Repaint "hello" at the home cell for ~1s, then go quiet.
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "printf hello; i=0; while [ $i -lt 20 ]; do printf '\\033[1;1Hhello'; \
             sleep 0.05; i=$((i+1)); done; sleep 5"
                .into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");

        let start = Instant::now();
        let idle = sess
            .wait_for_idle(Duration::from_millis(300), Duration::from_secs(10))
            .expect("wait");
        let elapsed = start.elapsed();
        sess.close();

        assert!(idle, "never reported idle");
        assert!(
            elapsed >= Duration::from_millis(800),
            "reported idle after {elapsed:?} while the child was still \
             repainting; screen-only comparison missed the raw output"
        );
    }

    /// A terminal has one stream; stderr is not separable from stdout.
    #[test]
    fn stderr_is_merged_into_the_terminal_stream() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "echo out; echo err 1>&2".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        assert!(sess
            .wait_for_text("err", Duration::from_secs(5))
            .expect("wait"));
        sess.close();
        let screen = sess.screen_text();
        assert!(screen.contains("out"), "stdout missing: {screen:?}");
        assert!(screen.contains("err"), "stderr missing: {screen:?}");
    }

    /// `Drop` must reap the child, and must not itself hang.
    #[cfg(unix)]
    #[test]
    fn drop_terminates_the_child_promptly() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "echo READY; sleep 30".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        assert!(sess
            .wait_for_text("READY", Duration::from_secs(5))
            .expect("wait"));
        let pid = sess.process_id().expect("pid");

        let start = Instant::now();
        drop(sess);
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "Drop blocked for {elapsed:?}"
        );

        // Give the kernel a moment to reap, then confirm the pid is gone.
        thread::sleep(Duration::from_millis(200));
        let alive = Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!alive, "child {pid} survived Drop");
    }

    /// [`PtySession::exit_signal`] reports a name, not just an exit code.
    ///
    /// A signalled child has no meaningful exit code — `From<std::process::
    /// ExitStatus>` gives it 1 — so the name is the only report of *how* it
    /// died. The name itself is `strsignal`'s and differs by platform
    /// (`"Hangup"`, `"Hangup: 1"`), so this checks that one was reported, not
    /// which.
    ///
    /// This does **not** enforce the `portable-pty` 0.9 floor, and is not
    /// claimed to. It asserts the public behaviour, and the 0.8 `Display`
    /// scrape the module docs describe produces the same behaviour, so this
    /// test passes on 0.8 as well (#37). What holds the floor is the typed
    /// `ExitStatus::signal()` call in `is_alive`, which will not compile
    /// there. Two separate guarantees, worth not conflating.
    #[cfg(unix)]
    #[test]
    fn a_signalled_child_reports_the_signal_by_name() {
        let config = PtyConfig::new(vec![
            "sh".into(),
            "-c".into(),
            "echo READY; sleep 30".into(),
        ]);
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        assert!(sess
            .wait_for_text("READY", Duration::from_secs(5))
            .expect("wait"));

        sess.terminate().expect("terminate");
        let deadline = Instant::now() + Duration::from_secs(5);
        while sess.is_alive() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }

        let signal = sess.exit_signal();
        assert!(
            signal.is_some_and(|s| !s.is_empty()),
            "child was killed by a signal but reported none; exit_code={:?}",
            sess.exit_code()
        );
        sess.close();
    }

    #[test]
    fn cast_is_written_when_path_given() {
        let dir = std::env::temp_dir().join(format!("termproof-pty-cast-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("pty.cast");
        let _ = std::fs::remove_file(&path);
        let config = PtyConfig::new(vec!["sh".into(), "-c".into(), "echo cast_hello".into()])
            .with_cast_path(path.clone());
        let mut sess = PtySession::new(config, 80, 24).expect("new");
        sess.spawn().expect("spawn");
        let _ = sess
            .wait_for_text("cast_hello", Duration::from_secs(2))
            .expect("wait");
        sess.close();
        let content = std::fs::read_to_string(&path).expect("read cast");
        let mut lines = content.lines();
        let header: serde_json::Value =
            serde_json::from_str(lines.next().expect("header")).expect("header json");
        assert_eq!(header["version"], 2);
        let events: Vec<String> = lines.map(|s| s.to_string()).collect();
        assert!(!events.is_empty(), "no events");
        let joined = events.join("\n");
        assert!(
            joined.contains("cast_hello"),
            "cast missing hello: {joined:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
