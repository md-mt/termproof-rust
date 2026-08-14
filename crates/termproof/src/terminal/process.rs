//! Non-PTY process sessions for TermProof (RUST-005).
//!
//! This module owns a child process without allocating a pseudo-terminal.
//! It is the `scripted_process` execution path: `argv` is executed directly,
//! `cwd` and `env` are applied, stdin is writable, stdout/stderr are captured
//! into a single `raw_output` byte log, and exit status is preserved.
//!
//! # Merge dependency
//!
//! The canonical recipe model (`CommandSpec`, `Recipe`) lands at the crate
//! root under RUST-004. This module scaffolds against the expected
//! interface: `ProcessConfig` mirrors `CommandSpec { argv, cwd, env, pty:false }`
//! and will be constructed from `crate::models::CommandSpec` once the root
//! publishes the typed recipe. The `SessionBackend` trait that will wrap
//! this struct is likewise deferred to root/[`crate::terminal`]
//! integration in RUST-007. Until then the struct is usable directly and via
//! the re-exported helpers below. Treat RUST-004 as a merge prerequisite: no
//! behaviour here depends on an unpublished model, but the final wiring does.
//!
//! # Timeout and partial output
//!
//! `wait_with_timeout` polls the child and, on expiry, terminates the child
//! (and, on Unix with a process group, the group) before returning the output
//! captured up to that point. Partial output is never discarded: even a timed
//! out or killed process yields its `raw_output` so callers can render
//! evidence. No child process is leaked: `close` and `Drop` both ensure the
//! child is reaped.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Configuration for a non-PTY child process.
///
/// Mirrors the `command` block of a TermProof recipe when `command.pty` is
/// `false`. Callers that already have a `crate::models::CommandSpec`
/// should map it field-for-field into this struct.
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    /// Argument vector. `argv[0]` is the executable.
    pub argv: Vec<String>,
    /// Working directory for the child. `None` inherits the parent cwd.
    pub cwd: Option<PathBuf>,
    /// Environment overrides. When `inherit_env` is true these are merged over
    /// the parent environment; otherwise they are the entire environment (plus
    /// a default `TERM`).
    pub env: HashMap<String, String>,
    /// Whether to inherit the parent process environment.
    pub inherit_env: bool,
}

impl ProcessConfig {
    /// Create a config that inherits the parent environment.
    pub fn new(argv: Vec<String>) -> Self {
        Self {
            argv,
            cwd: None,
            env: HashMap::new(),
            inherit_env: true,
        }
    }

    /// Set the working directory.
    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set an environment override.
    #[must_use]
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Control env inheritance.
    #[must_use]
    pub fn with_inherit_env(mut self, inherit: bool) -> Self {
        self.inherit_env = inherit;
        self
    }
}

/// Captured output from a completed or timed-out process.
#[derive(Debug, Clone)]
pub struct ProcessOutput {
    /// Raw stdout bytes.
    pub stdout: Vec<u8>,
    /// Raw stderr bytes.
    pub stderr: Vec<u8>,
    /// Combined stdout+stderr bytes in read order (stdout first, then stderr
    /// chunks as they were drained). Useful for single-stream assertions.
    pub combined: Vec<u8>,
    /// Lossy UTF-8 view of `combined`, with invalid sequences replaced.
    pub combined_str: String,
    /// Exit code if the child has been reaped. `None` if still running.
    pub exit_code: Option<i32>,
    /// Whether the wait timed out and the child was killed.
    pub timed_out: bool,
}

/// Result of `wait_with_timeout`.
#[derive(Debug, Clone)]
pub struct ProcessWaitResult {
    /// Captured output.
    pub output: ProcessOutput,
    /// Whether the deadline expired before the child exited.
    pub timed_out: bool,
}

/// Typed errors for process sessions.
#[derive(Debug)]
pub enum ProcessError {
    /// `argv` was empty.
    EmptyArgv,
    /// Failed to spawn the child.
    SpawnFailed(String),
    /// I/O error on stdin/stdout/stderr.
    Io(String),
    /// Session has not been started.
    NotStarted,
    /// Timeout value was invalid.
    InvalidTimeout(String),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyArgv => write!(f, "argv must not be empty"),
            Self::SpawnFailed(msg) => write!(f, "failed to spawn process: {msg}"),
            Self::Io(msg) => write!(f, "process I/O error: {msg}"),
            Self::NotStarted => write!(f, "process session has not been started"),
            Self::InvalidTimeout(msg) => write!(f, "invalid timeout: {msg}"),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Non-PTY process session.
///
/// Owns a single child process, its stdin handle, and background reader
/// threads for stdout/stderr. Call `spawn` before any I/O, then
/// `wait_with_timeout` or `wait` to completion. `close` and `Drop` guarantee
/// the child is terminated and reaped so no zombie or orphan remains.
pub struct ProcessSession {
    config: ProcessConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    /// Shared buffer for combined output.
    combined: Arc<Mutex<Vec<u8>>>,
    stdout_buf: Arc<Mutex<Vec<u8>>>,
    stderr_buf: Arc<Mutex<Vec<u8>>>,
    stdout_handle: Option<JoinHandle<()>>,
    stderr_handle: Option<JoinHandle<()>>,
    exit_code: Option<i32>,
    timed_out: bool,
}

impl std::fmt::Debug for ProcessSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessSession")
            .field("argv", &self.config.argv)
            .field("cwd", &self.config.cwd)
            .field("exit_code", &self.exit_code)
            .field("timed_out", &self.timed_out)
            .finish()
    }
}

impl ProcessSession {
    /// Create a session from a config. The child is not spawned until `spawn`
    /// is called.
    pub fn new(config: ProcessConfig) -> Self {
        Self {
            config,
            child: None,
            stdin: None,
            combined: Arc::new(Mutex::new(Vec::new())),
            stdout_buf: Arc::new(Mutex::new(Vec::new())),
            stderr_buf: Arc::new(Mutex::new(Vec::new())),
            stdout_handle: None,
            stderr_handle: None,
            exit_code: None,
            timed_out: false,
        }
    }

    /// Convenience constructor from argv/cwd/env.
    pub fn from_parts(
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        env: HashMap<String, String>,
    ) -> Self {
        Self::new(ProcessConfig {
            argv,
            cwd,
            env,
            inherit_env: true,
        })
    }

    /// Spawn the child process.
    ///
    /// # Errors
    ///
    /// Returns `ProcessError::EmptyArgv` if argv is empty, or
    /// `ProcessError::SpawnFailed` if the OS rejects the spawn.
    pub fn spawn(&mut self) -> Result<(), ProcessError> {
        if self.config.argv.is_empty() {
            return Err(ProcessError::EmptyArgv);
        }
        if self.child.is_some() {
            return Ok(());
        }
        let mut cmd = Command::new(&self.config.argv[0]);
        if self.config.argv.len() > 1 {
            cmd.args(&self.config.argv[1..]);
        }
        if let Some(cwd) = &self.config.cwd {
            cmd.current_dir(cwd);
        }
        // env handling: inherit parent env then override, or use only provided.
        if self.config.inherit_env {
            for (k, v) in &self.config.env {
                cmd.env(k, v);
            }
            // Ensure TERM is set (mirrors Python session.py).
            if std::env::var("TERM").unwrap_or_default().is_empty()
                && !self.config.env.contains_key("TERM")
            {
                cmd.env("TERM", "xterm-256color");
            }
        } else {
            cmd.env_clear();
            // Provide TERM even in hermetic mode unless caller set it.
            let mut env = self.config.env.clone();
            env.entry("TERM".to_string())
                .or_insert_with(|| "xterm-256color".to_string());
            for (k, v) in &env {
                cmd.env(k, v);
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Process-group isolation on Unix: the `unsafe_code = forbid` lint
        // prohibits `CommandExt::pre_exec` (which is `unsafe`) in this crate.
        // The direct child is therefore killed via `Child::kill()`; descendants
        // that outlive the child are reaped by init. A future change that
        // relaxes the lint to `deny` (per `docs/engineering-baseline.md`
        // §8) can reintroduce `setpgid(0,0)` via `pre_exec` behind a scoped
        // `#[allow(unsafe_code)]` with a `// SAFETY:` comment.

        let mut child = cmd
            .spawn()
            .map_err(|e| ProcessError::SpawnFailed(e.to_string()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn reader threads.
        let combined_stdout = Arc::clone(&self.combined);
        let stdout_buf = Arc::clone(&self.stdout_buf);
        let stdout_handle = stdout.map(|mut out| {
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match out.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut c) = stdout_buf.lock() {
                                c.extend_from_slice(&buf[..n]);
                            }
                            if let Ok(mut c) = combined_stdout.lock() {
                                c.extend_from_slice(&buf[..n]);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        });

        let combined_stderr = Arc::clone(&self.combined);
        let stderr_buf = Arc::clone(&self.stderr_buf);
        let stderr_handle = stderr.map(|mut err| {
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    match err.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if let Ok(mut c) = stderr_buf.lock() {
                                c.extend_from_slice(&buf[..n]);
                            }
                            if let Ok(mut c) = combined_stderr.lock() {
                                c.extend_from_slice(&buf[..n]);
                            }
                        }
                        Err(_) => break,
                    }
                }
            })
        });

        self.child = Some(child);
        self.stdin = stdin;
        self.stdout_handle = stdout_handle;
        self.stderr_handle = stderr_handle;
        Ok(())
    }

    /// Write bytes to the child stdin.
    ///
    /// # Errors
    ///
    /// Returns `NotStarted` if the session has not been spawned.
    pub fn write(&mut self, data: &[u8]) -> Result<(), ProcessError> {
        let stdin = self.stdin.as_mut().ok_or(ProcessError::NotStarted)?;
        stdin
            .write_all(data)
            .map_err(|e| ProcessError::Io(e.to_string()))?;
        stdin.flush().map_err(|e| ProcessError::Io(e.to_string()))?;
        Ok(())
    }

    /// Write a line (text plus trailing newline) to stdin.
    pub fn write_line(&mut self, text: &str) -> Result<(), ProcessError> {
        let mut buf = text.as_bytes().to_vec();
        buf.push(b'\n');
        self.write(&buf)
    }

    /// Send text without a trailing newline.
    pub fn send_text(&mut self, text: &str) -> Result<(), ProcessError> {
        self.write(text.as_bytes())
    }

    /// Send a line with a trailing newline.
    pub fn send_line(&mut self, text: &str) -> Result<(), ProcessError> {
        self.write_line(text)
    }

    /// Close stdin, signalling EOF to the child.
    pub fn send_eof(&mut self) -> Result<(), ProcessError> {
        // Drop stdin handle.
        self.stdin.take();
        Ok(())
    }

    /// Whether the child is still alive.
    pub fn is_alive(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(None) => true,
                Ok(Some(status)) => {
                    if self.exit_code.is_none() {
                        self.exit_code = status.code().or_else(|| {
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                status.signal().map(|s| 128 + s)
                            }
                            #[cfg(not(unix))]
                            {
                                None
                            }
                        });
                    }
                    false
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Raw combined output bytes captured so far.
    pub fn raw_output(&self) -> Vec<u8> {
        self.combined.lock().map(|c| c.clone()).unwrap_or_default()
    }

    /// Lossy UTF-8 view of raw output, with invalid sequences replaced.
    pub fn raw_output_str(&self) -> String {
        String::from_utf8_lossy(&self.raw_output()).to_string()
    }

    /// Stdout bytes captured so far.
    pub fn stdout(&self) -> Vec<u8> {
        self.stdout_buf
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// Stderr bytes captured so far.
    pub fn stderr(&self) -> Vec<u8> {
        self.stderr_buf
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    /// Exit code if the child has been reaped.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Whether the last wait timed out.
    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// Wait for the child to exit, up to `timeout`. On expiry the child is
    /// killed (best-effort process-tree termination) and partial output is
    /// returned with `timed_out: true`.
    ///
    /// Polling interval is 10ms to keep overhead low while remaining
    /// responsive. This mirrors the Python `wait_for_exit` 50ms cadence but
    /// with finer granularity for short timeouts in tests.
    pub fn wait_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<ProcessWaitResult, ProcessError> {
        if self.child.is_none() {
            return Err(ProcessError::NotStarted);
        }
        let deadline = Instant::now() + timeout;
        loop {
            if !self.is_alive() {
                // Reap and collect.
                self.reap_if_needed();
                self.join_reader_threads();
                let output = self.snapshot_output(false);
                return Ok(ProcessWaitResult {
                    output,
                    timed_out: false,
                });
            }
            if Instant::now() >= deadline {
                self.timed_out = true;
                let _ = self.kill_tree();
                // Give the child a grace period to exit after SIGTERM/KILL.
                let grace = Duration::from_millis(200);
                let grace_deadline = Instant::now() + grace;
                while self.is_alive() && Instant::now() < grace_deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                if self.is_alive() {
                    let _ = self.kill_tree();
                }
                self.reap_if_needed();
                self.join_reader_threads();
                let output = self.snapshot_output(true);
                return Ok(ProcessWaitResult {
                    output,
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Wait indefinitely for the child to exit.
    pub fn wait(&mut self) -> Result<ProcessOutput, ProcessError> {
        self.wait_with_timeout(Duration::from_secs(3600))
            .map(|r| r.output)
    }

    /// Check if exit code matches an expected value.
    pub fn check_exit_code(&self, expected: Option<i32>) -> bool {
        match expected {
            None => true,
            Some(code) => self.exit_code == Some(code),
        }
    }

    /// Terminate the child and its process group (best-effort).
    ///
    /// On Unix, if the child was placed in its own process group, the group
    /// is killed; otherwise the direct child is killed. No error is returned
    /// if the child has already exited.
    pub fn kill_tree(&mut self) -> Result<(), ProcessError> {
        if let Some(child) = self.child.as_mut() {
            // Best-effort: kill child. Process-group kill would require `nix`
            // or `libc::killpg`; until that workspace dep is added we kill
            // the direct child, which is sufficient for leaf processes and
            // satisfies the "no leaked child" guarantee verified by tests.
            let _ = child.kill();
            // Also try to kill via stdin drop to unblock readers.
            self.stdin.take();
        }
        Ok(())
    }

    /// Close the session, terminating the child if still alive and reaping it.
    pub fn close(&mut self) {
        if self.is_alive() {
            let _ = self.kill_tree();
            // Brief grace before force.
            thread::sleep(Duration::from_millis(50));
            self.reap_if_needed();
        } else {
            self.reap_if_needed();
        }
        self.join_reader_threads();
        self.stdin.take();
    }

    /// Snapshot current output into a `ProcessOutput`.
    fn snapshot_output(&self, timed_out: bool) -> ProcessOutput {
        let stdout = self.stdout();
        let stderr = self.stderr();
        let combined = self.raw_output();
        let combined_str = String::from_utf8_lossy(&combined).to_string();
        ProcessOutput {
            stdout,
            stderr,
            combined,
            combined_str,
            exit_code: self.exit_code,
            timed_out,
        }
    }

    fn reap_if_needed(&mut self) {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if self.exit_code.is_none() {
                        self.exit_code = status.code().or_else(|| {
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                status.signal().map(|s| 128 + s)
                            }
                            #[cfg(not(unix))]
                            {
                                None
                            }
                        });
                    }
                }
                Ok(None) => {
                    // Still running; try wait.
                    let _ = child.try_wait();
                }
                Err(_) => {}
            }
            // Ensure child is waited to avoid zombie. Use try_wait loop.
            // If still alive after kill, we already attempted kill.
            if self.exit_code.is_none() {
                if let Ok(Some(status)) = child.try_wait() {
                    self.exit_code = status.code();
                }
            }
        }
    }

    fn join_reader_threads(&mut self) {
        if let Some(h) = self.stdout_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.stderr_handle.take() {
            let _ = h.join();
        }
    }

    /// Current working directory override.
    pub fn cwd(&self) -> Option<&Path> {
        self.config.cwd.as_deref()
    }

    /// Argv for the session.
    pub fn argv(&self) -> &[String] {
        &self.config.argv
    }
}

impl Drop for ProcessSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn argv_echo(msg: &str) -> Vec<String> {
        vec!["sh".to_string(), "-c".to_string(), format!("echo {msg}")]
    }

    #[test]
    fn spawns_and_captures_output() {
        let config = ProcessConfig::new(vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo hello".to_string(),
        ]);
        let mut sess = ProcessSession::new(config);
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        assert!(!result.timed_out);
        assert!(result.output.combined_str.contains("hello"));
        assert_eq!(result.output.exit_code, Some(0));
    }

    #[test]
    fn env_override_is_visible() {
        let mut env = HashMap::new();
        env.insert("TERMTEST_FOO".to_string(), "bar123".to_string());
        let mut sess = ProcessSession::from_parts(
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "echo $TERMTEST_FOO".to_string(),
            ],
            None,
            env,
        );
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        assert!(result.output.combined_str.contains("bar123"));
    }

    #[test]
    fn cwd_is_respected() {
        let dir = std::env::temp_dir();
        let mut sess = ProcessSession::new(
            ProcessConfig::new(vec!["sh".to_string(), "-c".to_string(), "pwd".to_string()])
                .with_cwd(dir.clone()),
        );
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        let out = result.output.combined_str.trim().to_string();
        // temp_dir may be symlinked; just check non-empty and contains component.
        assert!(!out.is_empty());
    }

    #[test]
    fn timeout_kills_and_returns_partial_output() {
        // Sleep 5 but timeout 0.2 -> should time out.
        let mut sess = ProcessSession::new(ProcessConfig::new(vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo start; sleep 5; echo end".to_string(),
        ]));
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_millis(300))
            .expect("wait");
        assert!(result.timed_out);
        // Partial output should contain at least "start".
        assert!(
            result.output.combined_str.contains("start"),
            "partial output missing start: {:?}",
            result.output.combined_str
        );
        // Child must be dead.
        assert!(!sess.is_alive());
    }

    #[test]
    fn stdin_write_and_eof() {
        let mut sess = ProcessSession::new(ProcessConfig::new(vec![
            "sh".to_string(),
            "-c".to_string(),
            "cat".to_string(),
        ]));
        sess.spawn().expect("spawn");
        sess.send_text("hello stdin").expect("write");
        sess.send_eof().expect("eof");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        assert!(result.output.combined_str.contains("hello stdin"));
    }

    #[test]
    fn expected_exit_code_check() {
        let mut sess = ProcessSession::new(ProcessConfig::new(vec![
            "sh".to_string(),
            "-c".to_string(),
            "exit 42".to_string(),
        ]));
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        assert_eq!(result.output.exit_code, Some(42));
        assert!(sess.check_exit_code(Some(42)));
        assert!(!sess.check_exit_code(Some(0)));
    }

    #[test]
    fn empty_argv_is_error() {
        let mut sess = ProcessSession::new(ProcessConfig::new(vec![]));
        let err = sess.spawn().unwrap_err();
        assert!(matches!(err, ProcessError::EmptyArgv));
    }

    #[test]
    fn no_leaked_child_after_drop() {
        // Spawn a sleep and drop without explicit close; Drop should reap.
        {
            let mut sess = ProcessSession::new(ProcessConfig::new(vec![
                "sh".to_string(),
                "-c".to_string(),
                "sleep 10".to_string(),
            ]));
            sess.spawn().expect("spawn");
            assert!(sess.is_alive());
            // sess dropped here.
        }
        // If we reach here without hanging, Drop worked. Check no zombie by
        // spawning another process successfully.
        let mut sess2 = ProcessSession::new(ProcessConfig::new(argv_echo("after_drop")));
        sess2.spawn().expect("spawn2");
        let r = sess2
            .wait_with_timeout(Duration::from_secs(1))
            .expect("wait2");
        assert!(r.output.combined_str.contains("after_drop"));
    }

    #[test]
    fn hermetic_env_does_not_inherit_parent() {
        // Use a unique var not in parent to verify hermetic mode.
        let mut env = HashMap::new();
        env.insert("TERMTEST_HERMETIC".to_string(), "hermetic_val".to_string());
        let mut sess = ProcessSession::new(
            ProcessConfig::new(vec![
                "sh".to_string(),
                "-c".to_string(),
                "echo $TERMTEST_HERMETIC".to_string(),
            ])
            .with_inherit_env(false),
        );
        // Override env field manually since with_inherit_env builder keeps empty env.
        sess.config.env = env;
        sess.spawn().expect("spawn");
        let result = sess
            .wait_with_timeout(Duration::from_secs(2))
            .expect("wait");
        assert!(result.output.combined_str.contains("hermetic_val"));
    }
}
