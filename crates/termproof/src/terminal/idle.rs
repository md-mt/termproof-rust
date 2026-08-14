//! Idle detection without the hidden 3-second cap (fixes #77).
//!
//! The idle clock uses `Instant` deadlines and a condition driven by output
//! events. `wait_for_idle` returns `true` when the screen has been stable
//! for `stable_secs` within `timeout_secs`.  There is no unrelated hard
//! three-second ceiling — callers pass the real timeout they mean.

use std::time::{Duration, Instant};

/// Idle state tracker.  Feed it with `observe(current_screen)` and poll
/// `is_stable`.
#[derive(Debug)]
pub struct IdleTracker {
    stable_needed: Duration,
    deadline: Instant,
    last_screen: Option<String>,
    stable_since: Instant,
}

impl IdleTracker {
    /// Create a tracker that requires `stable_secs` of stability before `timeout_secs` elapses.
    pub fn new(stable_secs: f64, timeout_secs: f64) -> Self {
        let stable_needed = Duration::from_secs_f64(stable_secs.max(0.0));
        let timeout = Duration::from_secs_f64(timeout_secs.max(0.0));
        let now = Instant::now();
        Self {
            stable_needed,
            deadline: now + timeout,
            last_screen: None,
            stable_since: now,
        }
    }

    /// Observe a new screen snapshot at `now`.  Returns `true` if stable long enough.
    pub fn observe(&mut self, screen: &str, now: Instant) -> bool {
        if self.last_screen.as_deref() != Some(screen) {
            self.last_screen = Some(screen.to_string());
            self.stable_since = now;
        }
        now.duration_since(self.stable_since) >= self.stable_needed
    }

    /// Whether the deadline has passed.
    pub fn expired(&self, now: Instant) -> bool {
        now >= self.deadline
    }

    /// Remaining time until deadline.
    pub fn remaining(&self, now: Instant) -> Duration {
        self.deadline.saturating_duration_since(now)
    }
}

/// Block until `is_stable` returns true or timeout expires, polling every 50ms.
///
/// `poll` is called each iteration to probe the current screen.  This mirrors
/// Python's `session.wait_for_idle(stable_seconds, timeout_seconds)` but
/// without the erroneous `min(3, timeout)` cap that was applied in
/// `runner.py::_run_pty`.
pub fn wait_for_idle<F>(stable_secs: f64, timeout_secs: f64, mut poll: F) -> bool
where
    F: FnMut() -> String,
{
    let mut tracker = IdleTracker::new(stable_secs, timeout_secs);
    let start = Instant::now();
    // Initial observation
    let screen = poll();
    if tracker.observe(&screen, Instant::now()) && stable_secs == 0.0 {
        return true;
    }
    while !tracker.expired(Instant::now()) {
        std::thread::sleep(Duration::from_millis(50));
        let now = Instant::now();
        let screen = poll();
        if tracker.observe(&screen, now) {
            return true;
        }
        // Safety: if we've been running longer than timeout, break.
        if now.duration_since(start).as_secs_f64() >= timeout_secs {
            break;
        }
    }
    false
}
