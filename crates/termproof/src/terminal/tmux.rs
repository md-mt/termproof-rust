//! A [`Session`] backed by a real terminal, via tmux.
//!
//! The pty backend reconstructs the screen: bytes go through an emulator here
//! in-process, and what you read back is this crate's model of what a terminal
//! *would* show. That is fast, hermetic, and only as accurate as the emulator.
//!
//! This backend does the opposite. The program runs in a tmux pane, tmux owns
//! the grid, and `capture-pane` reads out what a terminal actually rendered.
//! When the two disagree, tmux is the tie-breaker — which makes it the right
//! tool for exactly two jobs:
//!
//! - **checking the emulator.** Run a recipe both ways; a difference is an
//!   emulation gap, and finding those any other way means reading escape
//!   sequences by hand.
//! - **applications the emulator mishandles.** Alternate screen, scroll
//!   regions, mouse reporting, anything unusual enough to be worth not
//!   modelling.
//!
//! It is slower — every read is a subprocess — and it needs `tmux` installed.
//! Use [`crate::terminal::pty_backend`] unless you have one of those two reasons.
//!
//! # Isolation
//!
//! Each session gets a private tmux socket under its own directory, so
//! concurrent runs cannot see each other's panes and a stuck session cannot
//! poison the next one. Teardown is `kill-server`, not `kill-session`: the
//! former reaps the daemon too, so a long run cannot leak one tmux process per
//! recipe.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Output;
use std::time::Duration;
use std::time::Instant;

use crate::terminal::backend::SessionBackend;
use crate::terminal::error::SessionError;
use crate::terminal::proc::run_with_timeout;
use crate::terminal::proc::sleep_secs;
use crate::terminal::session::Session;

const TMUX: &str = "tmux";
const SESSION: &str = "termproof";
const TMUX_TIMEOUT: Duration = Duration::from_secs(30);
const KILL_SERVER_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_SECONDS: f64 = 0.5;

/// Named keys, translated to tmux key names.
///
/// `send-keys -l` sends text literally and does not reliably carry control
/// bytes, so named keys have to be translated rather than written through. A
/// miss here is silent — the key becomes a no-op and the caller sees a failed
/// assertion rather than an error — which is why the table is tested directly.
const KEYS: [(&str, &str); 13] = [
    ("enter", "Enter"),
    ("tab", "Tab"),
    ("escape", "Escape"),
    ("backspace", "BSpace"),
    ("up", "Up"),
    ("down", "Down"),
    ("left", "Left"),
    ("right", "Right"),
    ("ctrl-c", "C-c"),
    ("ctrl-d", "C-d"),
    ("ctrl-p", "C-p"),
    ("ctrl-[", "Escape"),
    ("f13", "F13"),
];

/// Translate a named key to its tmux spelling.
fn tmux_key(key: &str) -> Option<&'static str> {
    let lowered = key.to_ascii_lowercase();
    KEYS.iter()
        .find(|(name, _)| *name == lowered)
        .map(|(_, tmux)| *tmux)
}

/// A session running inside a tmux pane.
pub struct TmuxSession {
    socket: String,
    argv: Vec<String>,
    cast_path: PathBuf,
    /// The directory the pane was created in, when that is knowable. `None`
    /// when `new-session -c` was handed a path that is not a directory: tmux
    /// takes it, puts the pane somewhere else, and does not say where.
    workdir: Option<PathBuf>,
    cols: u16,
    rows: u16,
    exit_code: Option<i32>,
    closed: bool,
    /// Last captured screen, so [`Session::screen`] can return a borrow.
    screen: String,
    /// Last captured screen including escapes.
    raw: String,
}

impl TmuxSession {
    /// Run a tmux command against this session's private socket.
    ///
    /// On timeout the server is killed before the error propagates, so a hung
    /// tmux is not left orphaned. That kill runs `Command` directly rather than
    /// recursing here: if the server is wedged, a recursive call would hit the
    /// same timeout and could recurse again.
    fn tmux(&self, args: &[&str]) -> Result<Output, SessionError> {
        let mut cmd = Command::new(TMUX);
        cmd.arg("-S").arg(&self.socket).args(args);
        match run_with_timeout(cmd, TMUX_TIMEOUT) {
            Ok(output) if output.status.success() => Ok(output),
            Ok(output) => Err(SessionError::Io(format!(
                "tmux {:?} exited {}: {}",
                args,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim_end()
            ))),
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                let mut kill = Command::new(TMUX);
                kill.arg("-S").arg(&self.socket).arg("kill-server");
                let _ = run_with_timeout(kill, KILL_SERVER_TIMEOUT);
                Err(SessionError::Timeout(format!(
                    "tmux {args:?} timed out after {}s; server killed",
                    TMUX_TIMEOUT.as_secs()
                )))
            }
            Err(error) => Err(SessionError::Io(error.to_string())),
        }
    }

    /// Read the pane, with or without escape sequences.
    fn capture(&self, escapes: bool) -> String {
        let mut args = vec!["capture-pane", "-t", SESSION, "-p"];
        if escapes {
            args.push("-e");
        }
        self.tmux(&args)
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default()
    }

    /// Refresh the cached screen and raw output.
    fn refresh(&mut self) {
        self.screen = self.capture(false);
        self.raw = self.capture(true);
    }

    /// The part of the captured screen that can actually settle.
    ///
    /// Drops the last two rows: the tmux status line and the cursor row change
    /// on their own and would never let the screen look stable.
    fn settled_body(&self) -> String {
        let lines: Vec<&str> = self.screen.lines().collect();
        let keep = lines.len().saturating_sub(2);
        lines[..keep].join("\n")
    }
}

impl Session for TmuxSession {
    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        self.tmux(&["send-keys", "-t", SESSION, "-l", "--", text])?;
        sleep_secs(0.1);
        self.refresh();
        Ok(())
    }

    fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
        self.send_text(text)?;
        self.press("enter")
    }

    fn press(&mut self, key: &str) -> Result<(), SessionError> {
        let Some(name) = tmux_key(key) else {
            return Err(SessionError::Config(format!(
                "unknown key {key:?}; known keys: {}",
                KEYS.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(", ")
            )));
        };
        self.tmux(&["send-keys", "-t", SESSION, name])?;
        sleep_secs(0.1);
        self.refresh();
        Ok(())
    }

    fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError> {
        let deadline = Instant::now() + timeout;
        loop {
            self.refresh();
            if self.screen.contains(text) || self.raw.contains(text) {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            sleep_secs(POLL_SECONDS);
        }
    }

    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, SessionError> {
        let deadline = Instant::now() + timeout;
        self.refresh();
        let mut previous = self.settled_body();
        // Measure against the clock, not a count of polls. Adding POLL_SECONDS
        // per match credits an interval that has not elapsed yet, so a screen
        // that matches on the very first look satisfies any `stable` shorter
        // than one poll without having waited at all.
        let mut stable_since = Instant::now();
        loop {
            if stable_since.elapsed() >= stable {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            sleep_secs(POLL_SECONDS);
            self.refresh();
            let body = self.settled_body();
            if body != previous {
                previous = body;
                stable_since = Instant::now();
            }
        }
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !self.is_alive() {
                return Ok(self.exit_code);
            }
            sleep_secs(POLL_SECONDS);
        }
        Ok(None)
    }

    fn read_available(&mut self, _timeout: Duration) -> Result<(), SessionError> {
        // tmux owns the grid; there is no stream to drain. Refreshing the
        // cached capture is the equivalent operation.
        self.refresh();
        Ok(())
    }

    fn is_alive(&mut self) -> bool {
        if self.closed {
            return false;
        }
        self.tmux(&["has-session", "-t", SESSION]).is_ok()
    }

    fn close(&mut self) -> Result<(), SessionError> {
        if self.closed {
            return Ok(());
        }
        self.refresh();
        // kill-server, not kill-session: it reaps the daemon too, so a long run
        // cannot leak one tmux process per session.
        let _ = self.tmux(&["kill-server"]);
        self.closed = true;
        Ok(())
    }

    fn screen(&self) -> &str {
        &self.screen
    }

    fn raw_output(&self) -> &str {
        &self.raw
    }

    fn screen_attributed(&mut self) -> Option<crate::terminal::attributed::AttributedScreen> {
        // tmux already owns a real grid; `capture-pane -e` gives it back with
        // escapes, which the ANSI parser turns into cells. Nothing is being
        // re-emulated here — this is the rendered truth.
        self.refresh();
        Some(
            crate::terminal::attributed::attributed_screen_from_ansi_text(
                &self.raw,
                self.cols as usize,
                self.rows as usize,
            ),
        )
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

    fn cwd(&self) -> Option<&Path> {
        // The directory the pane was created in, reported as the recipe spelled
        // it — a relative one is relative to whoever started the run.
        //
        // tmux is the one backend that could answer the live question:
        // `display-message -p '#{pane_current_path}'` does follow a `cd` inside
        // the pane. It is not what this returns, for two reasons. It costs a
        // subprocess on every call, on a method whose every other implementor
        // is a field read; and it would make `cwd` mean "where it is now" here
        // and "where it started" everywhere else, which is worse than a caveat
        // that holds uniformly. A live reading is a separate method, and it
        // would have to return an owned path.
        self.workdir.as_deref()
    }

    fn cast_path(&self) -> &Path {
        &self.cast_path
    }
}

impl Drop for TmuxSession {
    /// A dropped session must not leave a tmux server running.
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.close();
        }
    }
}

/// Creates [`TmuxSession`]s.
#[derive(Debug, Default, Clone, Copy)]
pub struct TmuxBackend;

impl SessionBackend for TmuxBackend {
    fn create_session(
        &self,
        argv: Vec<String>,
        cast_path: PathBuf,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Result<Box<dyn Session>, SessionError> {
        if argv.is_empty() {
            return Err(SessionError::Config("argv is empty".to_string()));
        }
        let dir = cast_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        fs::create_dir_all(&dir).map_err(|e| SessionError::Io(e.to_string()))?;

        // A private socket per session: concurrent runs cannot see each other's
        // panes, and a wedged session cannot poison the next one.
        let socket = dir.join("tmux.sock").to_string_lossy().to_string();
        let requested = launch_dir(cwd.as_deref(), &dir);
        let workdir_arg = requested.to_string_lossy().to_string();
        // What tmux is told and what the pane can be said to be in are not the
        // same thing. `new-session` does not refuse a `-c` that is not a
        // directory: it succeeds, creates the pane in the home directory
        // instead, and reports none of that. Pass the request through as
        // before, but only claim a directory we can see is one.
        let workdir = requested.is_dir().then_some(requested);
        let wrapper = write_launch_script(&dir, &env, &argv)?;

        let mut session = TmuxSession {
            socket,
            argv,
            cast_path,
            workdir,
            cols,
            rows,
            exit_code: None,
            closed: false,
            screen: String::new(),
            raw: String::new(),
        };

        let (cols_s, rows_s) = (cols.to_string(), rows.to_string());
        session.tmux(&[
            "new-session",
            "-d",
            "-s",
            SESSION,
            "-x",
            &cols_s,
            "-y",
            &rows_s,
            "-c",
            &workdir_arg,
            "sh",
            &wrapper,
        ])?;
        session.refresh();
        Ok(Box::new(session))
    }

    fn name(&self) -> &str {
        "tmux"
    }
}

/// What `new-session -c` is given: the directory the recipe named, or the
/// session's own directory when it named none.
///
/// The fallback is not the runner's directory. The launch script is written
/// into the session directory and the pane has to be able to reach it, and a
/// pane that starts beside its own artifacts is easier to inspect after a
/// failure than one that starts wherever the run happened to be invoked.
///
/// This is the request, not the outcome — see the caller for why the two are
/// not the same when the path does not exist.
fn launch_dir(cwd: Option<&str>, session_dir: &Path) -> PathBuf {
    cwd.map_or_else(|| session_dir.to_path_buf(), PathBuf::from)
}

/// Write a shell wrapper that exports `env` and then execs the command.
///
/// tmux takes a command line, not an environment, so the environment has to be
/// carried in the script. Writing a file rather than building one long `-c`
/// string keeps quoting tractable and makes the launch inspectable after a
/// failure — the script is still on disk.
fn write_launch_script(
    dir: &Path,
    env: &HashMap<String, String>,
    argv: &[String],
) -> Result<String, SessionError> {
    let mut script = String::from("#!/bin/sh\n");
    // Sorted so the script is byte-stable across runs: a wrapper that differs
    // only in export order is noise when diffing two failed launches.
    let mut keys: Vec<&String> = env.keys().collect();
    keys.sort();
    for k in keys {
        script.push_str(&format!("export {}={}\n", k, shell_quote(&env[k])));
    }
    script.push_str(&format!("exec {}\n", shell_join(argv)));

    let path = dir.join("launch.sh");
    fs::write(&path, script).map_err(|e| SessionError::Io(e.to_string()))?;
    let mut perms = fs::metadata(&path)
        .map_err(|e| SessionError::Io(e.to_string()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).map_err(|e| SessionError::Io(e.to_string()))?;
    Ok(path.to_string_lossy().to_string())
}

/// Quote one argument for `sh`.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '=' | ':' | '-' | ',')
        })
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Quote and join arguments into one `sh` command line.
pub fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_keys_translate() {
        assert_eq!(tmux_key("enter"), Some("Enter"));
        assert_eq!(tmux_key("ctrl-c"), Some("C-c"));
        assert_eq!(tmux_key("backspace"), Some("BSpace"));
    }

    #[test]
    fn key_lookup_is_case_insensitive() {
        assert_eq!(tmux_key("Enter"), Some("Enter"));
        assert_eq!(tmux_key("CTRL-D"), Some("C-d"));
    }

    #[test]
    fn an_unknown_key_is_none_rather_than_a_guess() {
        // The original silently dropped unmapped keys, which turns a typo into
        // a failed assertion three steps later instead of an error here.
        assert_eq!(tmux_key("hyperspace"), None);
    }

    #[test]
    fn ctrl_bracket_maps_to_escape() {
        // Terminals send the same byte for both; tmux names it Escape.
        assert_eq!(tmux_key("ctrl-["), Some("Escape"));
    }

    #[test]
    fn plain_arguments_are_left_alone() {
        assert_eq!(shell_quote("/bin/app"), "/bin/app");
        assert_eq!(shell_quote("--flag=value"), "--flag=value");
    }

    #[test]
    fn arguments_needing_quotes_get_them() {
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn embedded_single_quotes_survive() {
        // The classic quoting bug: a naive wrapper ends the string early and
        // the rest of the argument becomes shell syntax.
        assert_eq!(shell_quote("it's"), r#"'it'\''s'"#);
    }

    #[test]
    fn joining_quotes_each_argument() {
        let args = vec!["/bin/app".to_string(), "a b".to_string()];
        assert_eq!(shell_join(&args), "/bin/app 'a b'");
    }

    #[test]
    fn the_launch_script_exports_then_execs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut env = HashMap::new();
        env.insert("B".to_string(), "two words".to_string());
        env.insert("A".to_string(), "one".to_string());
        let path = write_launch_script(
            dir.path(),
            &env,
            &["/bin/app".to_string(), "--x".to_string()],
        )
        .expect("script written");
        let body = fs::read_to_string(&path).expect("readable");

        assert!(body.starts_with("#!/bin/sh\n"), "{body}");
        // Sorted, so two failed launches diff cleanly.
        let a = body.find("export A=").expect("A exported");
        let b = body.find("export B=").expect("B exported");
        assert!(a < b, "exports should be sorted:\n{body}");
        assert!(body.contains("export B='two words'"), "{body}");
        // exec, not a plain call: the wrapper must not linger as a parent.
        assert!(body.trim_end().ends_with("exec /bin/app --x"), "{body}");
    }

    #[test]
    fn the_launch_script_is_executable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_launch_script(dir.path(), &HashMap::new(), &["/bin/true".to_string()])
            .expect("script written");
        let mode = fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "should be executable, got {mode:o}");
    }

    #[test]
    fn an_empty_argv_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        // `expect_err` would need `Box<dyn Session>: Debug`, which it is not.
        let Err(err) = TmuxBackend.create_session(
            vec![],
            dir.path().join("s.cast"),
            None,
            HashMap::new(),
            80,
            24,
        ) else {
            panic!("empty argv should be refused");
        };
        assert!(matches!(err, SessionError::Config(_)), "{err:?}");
    }

    /// A session whose socket does not exist: every `tmux` call fails, so each
    /// capture reads as an empty screen. That is the cheapest way to hold the
    /// screen perfectly still and time what `wait_for_idle` does about it.
    fn unreachable_session(dir: &Path) -> TmuxSession {
        TmuxSession {
            socket: dir.join("absent.sock").to_string_lossy().to_string(),
            argv: vec!["/bin/true".to_string()],
            cast_path: dir.join("s.cast"),
            workdir: Some(dir.to_path_buf()),
            cols: 80,
            rows: 24,
            exit_code: None,
            // Already closed, so `Drop` does not try to kill a server that was
            // never started.
            closed: true,
            screen: String::new(),
            raw: String::new(),
        }
    }

    #[test]
    fn idle_waits_out_the_stable_window_rather_than_counting_polls() {
        // Counting `steady += POLL_SECONDS` per matching poll credits an
        // interval that has not elapsed. A screen that matches on the first
        // look then reports "stable for half a second" immediately, and a step
        // that asked to see the screen hold still gets no wait at all.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = unreachable_session(dir.path());
        let stable = Duration::from_millis(400);

        let started = Instant::now();
        let idle = session
            .wait_for_idle(stable, Duration::from_secs(10))
            .expect("no error");

        assert!(idle, "an unchanging screen is idle");
        assert!(
            started.elapsed() >= stable,
            "returned after {:?}, less than the {stable:?} it was asked to wait",
            started.elapsed()
        );
    }

    #[test]
    fn the_backend_names_itself() {
        assert_eq!(TmuxBackend.name(), "tmux");
    }

    #[test]
    fn the_pane_starts_where_the_recipe_said() {
        assert_eq!(
            launch_dir(Some("/srv/app"), Path::new("/tmp/run-1")),
            PathBuf::from("/srv/app")
        );
    }

    #[test]
    fn a_recipe_with_no_directory_starts_the_pane_beside_its_artifacts() {
        // The session directory, not the runner's: the launch script lives
        // there and the pane has to be able to reach it.
        assert_eq!(
            launch_dir(None, Path::new("/tmp/run-1")),
            PathBuf::from("/tmp/run-1")
        );
    }

    #[test]
    fn a_pane_reports_the_directory_it_was_created_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let session = unreachable_session(dir.path());
        assert_eq!(Session::cwd(&session), Some(dir.path()));
    }

    #[test]
    fn a_directory_that_does_not_exist_is_still_passed_to_tmux() {
        // Whether to accept a bad `-c` is tmux's call, not ours, and it does:
        // `new-session` succeeds. Only the reporting changes.
        let requested = launch_dir(Some("/no/such/place"), Path::new("/tmp/run-1"));
        assert_eq!(requested, PathBuf::from("/no/such/place"));
        assert!(
            !requested.is_dir(),
            "the filter the caller applies is what decides `cwd`"
        );
    }

    #[test]
    fn a_pane_sent_somewhere_that_does_not_exist_reports_nothing() {
        // tmux takes the `-c`, creates the pane in the home directory instead,
        // and says nothing about it. Echoing the request back would name a
        // directory the pane was never in, so this answers "cannot say".
        let dir = tempfile::tempdir().expect("tempdir");
        let mut session = unreachable_session(dir.path());
        session.workdir = None;
        assert_eq!(Session::cwd(&session), None);
    }
}
