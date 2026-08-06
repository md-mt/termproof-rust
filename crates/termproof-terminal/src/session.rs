//! Public Session trait and a deterministic mock for unit/corpus tests.
//!
//! The real PTY and process backends (RUST-005/006) will implement `Session`
//! on top of portable-pty + vt100 and share the same contract. Steps only see
//! this trait, so RUST-007 can ship steps + corpus parity without blocking
//! on those crates.

use std::time::{Duration, Instant};

/// Minimal terminal session contract that the seven built-in steps need.
///
/// The trait mirrors `termproof/session.py:TerminalSession`'s public surface
/// used by `builtin_steps.py`:
///
/// * `screen` + `raw_output` are the two buffers `wait_for_text` and
///   `wait_for_regex` search *independently* (never concatenated across a
///   synthetic `\\n` boundary).
/// * `is_alive` / `read_available` let `wait_for_*` poll without busy-waiting
///   and detect process exit.
/// * `send_text` / `send_line` / `press` feed input; `sleep` ticks the session
///   via `read_available(0)` as the Python oracle does.
pub trait Session {
    /// Current VT screen text, r-stripped and without trailing blank lines
    /// (same as `screen_text()` in Python's `screen.py`).
    fn screen(&self) -> String;

    /// Uncooked terminal output bytes accumulated so far.
    fn raw_output(&self) -> &str;

    /// Whether the child process / PTY is still alive.
    fn is_alive(&self) -> bool;

    /// Poll for new output, blocking up to `timeout`.
    ///
    /// Real backends call `read_nonblocking` / PTY reads. The mock advances a
    /// synthetic event queue or just returns.
    fn read_available(&mut self, timeout: Duration);

    /// Append text to the child stdin without a trailing CR.
    fn send_text(&mut self, text: &str) -> Result<(), String>;

    /// Send `text + \"\\r\"` (matches Python `send_line` which appends `\\r`).
    fn send_line(&mut self, text: &str) -> Result<(), String> {
        self.send_text(&format!("{text}\r"))
    }

    /// Send a named key or `Ctrl-` combo.
    fn press(&mut self, key: &str) -> Result<(), String>;

    // ---- derived wait helpers (spec §5.3: Instant-based deadlines) ----

    /// Poll until `needle` appears in `screen()` or `raw_output()`, or the
    /// deadline expires. Returns `true` on match, `false` on timeout/exit.
    fn wait_for_text(&mut self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.read_available(Duration::from_millis(50));
            if self.screen().contains(needle) || self.raw_output().contains(needle) {
                return true;
            }
            if !self.is_alive() {
                self.read_available(Duration::ZERO);
                return self.screen().contains(needle) || self.raw_output().contains(needle);
            }
        }
        false
    }

    /// Poll until the screen has been stable for `stable`, or `timeout`
    /// expires. Returns `true` when stable, `false` on timeout. If the child
    /// exits, the wait is satisfied (matches Python: `return True` on exit).
    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut last = self.screen();
        let mut stable_since = Instant::now();
        while Instant::now() < deadline {
            self.read_available(Duration::from_millis(50));
            let cur = self.screen();
            if cur != last {
                last = cur;
                stable_since = Instant::now();
            }
            if Instant::now().duration_since(stable_since) >= stable {
                return true;
            }
            if !self.is_alive() {
                self.read_available(Duration::ZERO);
                return true;
            }
        }
        false
    }
}

/// Deterministic mock for unit tests. Screen and raw buffers are seeded by the
/// test and `expected_*` events can be enqueued to simulate streaming output.
#[derive(Debug, Default)]
pub struct MockSession {
    screen_text: String,
    raw: String,
    alive: bool,
    sent: Vec<String>,
    /// How many `read_available` calls before queued events fire.
    pending_ticks: usize,
    /// Events that will be appended to `screen_text`/`raw` after pending_ticks.
    queued: Vec<(String, String)>,
}

impl MockSession {
    /// Create a live session with given initial screen/raw.
    pub fn new(screen: impl Into<String>, raw: impl Into<String>) -> Self {
        Self {
            screen_text: screen.into(),
            raw: raw.into(),
            alive: true,
            sent: Vec::new(),
            pending_ticks: 0,
            queued: Vec::new(),
        }
    }

    /// Mark the session as dead (simulates process exit).
    pub fn set_alive(&mut self, alive: bool) {
        self.alive = alive;
    }

    /// Enqueue a screen/raw delta that fires after `ticks` polls.
    pub fn enqueue(
        &mut self,
        screen_delta: impl Into<String>,
        raw_delta: impl Into<String>,
        ticks: usize,
    ) {
        self.queued.push((screen_delta.into(), raw_delta.into()));
        // Merge ticks: earliest fires first
        self.pending_ticks = ticks;
    }

    /// Directly overwrite screen (for simple test setup).
    pub fn set_screen(&mut self, s: impl Into<String>) {
        self.screen_text = s.into();
    }

    /// Directly overwrite raw buffer (for simple test setup).
    pub fn set_raw(&mut self, r: impl Into<String>) {
        self.raw = r.into();
    }

    /// What was sent via `send_text`/`press` (for assertions).
    pub fn sent(&self) -> &[String] {
        &self.sent
    }
}

impl Session for MockSession {
    fn screen(&self) -> String {
        self.screen_text.clone()
    }

    fn raw_output(&self) -> &str {
        &self.raw
    }

    fn is_alive(&self) -> bool {
        self.alive
    }

    fn read_available(&mut self, _timeout: Duration) {
        if self.pending_ticks > 0 {
            self.pending_ticks -= 1;
            if self.pending_ticks == 0 {
                for (sc, ra) in self.queued.drain(..) {
                    self.screen_text.push_str(&sc);
                    self.raw.push_str(&ra);
                }
            }
        }
        // Sleep-step contract: real sessions call read_available(0) after
        // `time.sleep` to drain any buffered output. Mock just returns.
    }

    fn send_text(&mut self, text: &str) -> Result<(), String> {
        if !self.alive {
            return Err("session not alive".into());
        }
        self.sent.push(text.to_string());
        Ok(())
    }

    fn press(&mut self, key: &str) -> Result<(), String> {
        use crate::keys::press_sequence;
        let action = press_sequence(key).map_err(|e| format!("{e:?}"))?;
        match action {
            crate::keys::PressAction::Ctrl(ch) => {
                self.sent.push(format!("ctrl-{ch}"));
                Ok(())
            }
            crate::keys::PressAction::Sequence(seq) => {
                self.sent.push(seq);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_text_finds_screen() {
        let mut s = MockSession::new("hello world", "");
        assert!(s.wait_for_text("world", Duration::from_millis(80)));
    }
    #[test]
    fn wait_for_text_times_out() {
        let mut s = MockSession::new("hello", "");
        assert!(!s.wait_for_text("MISSING", Duration::from_millis(80)));
    }
    #[test]
    fn wait_for_idle_stable() {
        let mut s = MockSession::new("stable\n", "");
        assert!(s.wait_for_idle(Duration::from_millis(20), Duration::from_millis(300)));
    }
}
