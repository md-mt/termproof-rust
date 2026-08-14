//! Asciinema v2 cast recording and activity clock (RUST-006).
//!
//! The cast format matches the Python oracle `termproof/cast.py` and
//! `termproof/session.py::asciinema_rec_command`: a JSON header line followed
//! by time-stamped `[delay, kind, data]` events. `kind` is `"o"` for output
//! and `"i"` for input. Delays are seconds since the recorder started (float,
//! six decimal places in Python, here as `f64`).
//!
//! `ActivityClock` tracks the last time output was observed, used by
//! `wait_for_idle` to determine when the screen has been stable.
//!
//! Merge note: RUST-004 provides `Recipe` paths that decide where the cast is
//! written. Until then `CastRecorder` accepts an explicit `PathBuf` and is
//! tested against fixture goldens for deterministic serialization.

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Header for an asciinema v2 cast file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastHeader {
    /// Cast format version, always 2.
    pub version: u8,
    /// Terminal width in columns.
    pub width: u16,
    /// Terminal height in rows.
    pub height: u16,
    /// Unix timestamp (seconds since epoch) when recording started.
    pub timestamp: u64,
    /// Command that was executed (joined argv) for human inspection.
    #[serde(default)]
    pub command: String,
    /// Environment snapshot (subset: `SHELL`, `TERM`).
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Optional title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A single cast event `[delay, kind, data]`.
#[derive(Debug, Clone)]
pub struct CastEvent {
    /// Seconds since recording started.
    pub delay: f64,
    /// `"o"` for output, `"i"` for input.
    pub kind: String,
    /// Payload.
    pub data: String,
}

/// Monotonic activity clock for `wait_for_idle`.
///
/// Records the instant of the last observed output or screen change. The
/// terminal session updates it on every `read_available`. Step code checks
/// `idle_duration()` against `stable_seconds`.
#[derive(Debug, Clone)]
pub struct ActivityClock {
    start: Instant,
    last_activity: Instant,
}

impl ActivityClock {
    /// Start the clock now.
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            start: now,
            last_activity: now,
        }
    }

    /// Mark activity now (e.g. new output or screen diff).
    pub fn mark(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Mark activity at a specific instant (for deterministic tests).
    pub fn mark_at(&mut self, at: Instant) {
        self.last_activity = at;
    }

    /// Duration since the last activity.
    pub fn idle_duration(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Duration at a given `now` (test helper, avoids wall-clock flakiness).
    pub fn idle_duration_at(&self, now: Instant) -> Duration {
        now.duration_since(self.last_activity)
    }

    /// Elapsed since the clock was created.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Delay since start as float seconds.
    pub fn delay_secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    /// Whether the clock has been idle for at least `stable`.
    pub fn is_idle_for(&self, stable: Duration) -> bool {
        self.idle_duration() >= stable
    }

    /// Whether the clock has been idle for `stable` at `now`.
    pub fn is_idle_for_at(&self, stable: Duration, now: Instant) -> bool {
        self.idle_duration_at(now) >= stable
    }
}

impl Default for ActivityClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Recorder that writes an asciinema v2 cast file.
///
/// Usage:
/// ```no_run
/// use termproof::terminal::cast::CastRecorder;
/// use std::path::PathBuf;
/// let mut rec = CastRecorder::new(PathBuf::from("/tmp/session.cast"), 80, 24, vec!["sh".into(), "-c".into(), "echo hi".into()]).unwrap();
/// rec.record_output("hi\n");
/// rec.finish().unwrap();
/// ```
#[derive(Debug)]
pub struct CastRecorder {
    path: PathBuf,
    cols: u16,
    rows: u16,
    #[allow(dead_code)]
    command: Vec<String>,
    start: Instant,
    file: Option<File>,
}

impl CastRecorder {
    /// Create a recorder and write the header immediately.
    ///
    /// The file is created (and truncated) at construction so the header is
    /// durable even if the child crashes before any output.
    pub fn new(
        path: PathBuf,
        cols: u16,
        rows: u16,
        command: Vec<String>,
    ) -> Result<Self, std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        let header = CastHeader {
            version: 2,
            width: cols,
            height: rows,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            command: command.join(" "),
            env: {
                let mut m = HashMap::new();
                m.insert(
                    "SHELL".to_string(),
                    std::env::var("SHELL").unwrap_or_default(),
                );
                m.insert(
                    "TERM".to_string(),
                    std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
                );
                m
            },
            title: None,
        };
        let line = serde_json::to_string(&header).unwrap_or_else(|_| "{}".to_string());
        writeln!(file, "{line}")?;
        file.flush()?;
        Ok(Self {
            path,
            cols,
            rows,
            command,
            start: Instant::now(),
            file: Some(file),
        })
    }

    /// Record output data (`"o"` event).
    pub fn record_output(&mut self, data: &str) {
        self.record("o", data);
    }

    /// Record input data (`"i"` event).
    pub fn record_input(&mut self, data: &str) {
        self.record("i", data);
    }

    fn record(&mut self, kind: &str, data: &str) {
        if data.is_empty() {
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let delay = self.start.elapsed().as_secs_f64();
        // Event is a JSON array [delay, kind, data].
        let event = serde_json::json!([delay, kind, data]);
        let line = serde_json::to_string(&event).unwrap_or_default();
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }

    /// Flush and close the file. The header + events are now durable.
    pub fn finish(&mut self) -> Result<(), std::io::Error> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        Ok(())
    }

    /// Path of the cast file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Dimensions.
    pub fn dimensions(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }
}

impl Drop for CastRecorder {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// Read a cast file and replay it into a `TerminalScreen`.
///
/// Returns `(screen_text, cols, rows)` matching `termproof/screen.py::replay_cast`.
pub fn replay_cast(path: &Path) -> Result<(String, u16, u16), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut header_line = String::new();
    reader.read_line(&mut header_line)?;
    let header: CastHeader = serde_json::from_str(&header_line)?;
    let mut screen = crate::terminal::screen::TerminalScreen::new(header.width, header.height);
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let event: serde_json::Value = serde_json::from_str(&line)?;
        if let Some(arr) = event.as_array() {
            if arr.len() >= 3 && arr[1].as_str() == Some("o") {
                if let Some(data) = arr[2].as_str() {
                    screen.feed_str(data);
                }
            }
        }
    }
    Ok((screen.contents(), header.width, header.height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn activity_clock_idle() {
        let mut clock = ActivityClock::new();
        // Immediately after mark, not idle for 1s.
        assert!(!clock.is_idle_for(Duration::from_secs(1)));
        // Simulate time passing by marking in the past.
        let past = Instant::now() - Duration::from_millis(600);
        clock.mark_at(past);
        // Now idle for 500ms should be true.
        assert!(clock.is_idle_for(Duration::from_millis(500)));
        assert!(!clock.is_idle_for(Duration::from_secs(1)));
    }

    #[test]
    fn activity_clock_deterministic_at() {
        let clock = ActivityClock {
            start: Instant::now(),
            last_activity: Instant::now() - Duration::from_millis(200),
        };
        let now = Instant::now();
        // idle_duration_at should be ~200ms.
        let idle = clock.idle_duration_at(now);
        assert!(idle >= Duration::from_millis(190));
        // Use at helper.
        let future = now + Duration::from_secs(1);
        assert!(clock.is_idle_for_at(Duration::from_millis(500), future));
    }

    #[test]
    fn cast_round_trip() {
        let dir = std::env::temp_dir().join(format!("termproof-cast-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.cast");
        let _ = std::fs::remove_file(&path);
        let mut rec =
            CastRecorder::new(path.clone(), 80, 24, vec!["echo".into(), "hi".into()]).unwrap();
        rec.record_output("hello\n");
        rec.record_input("hi\n");
        rec.record_output("world\n");
        rec.finish().unwrap();

        // Read back.
        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["version"], 2);
        assert_eq!(header["width"], 80);
        assert_eq!(header["height"], 24);
        // Next lines are events.
        let events: Vec<serde_json::Value> =
            lines.map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0][1], "o");
        assert_eq!(events[0][2], "hello\n");
        assert_eq!(events[1][1], "i");
        assert_eq!(events[2][2], "world\n");

        // Replay via screen should contain hello and world.
        let (text, cols, rows) = replay_cast(&path).unwrap();
        assert_eq!(cols, 80);
        assert_eq!(rows, 24);
        assert!(text.contains("hello"));
        assert!(text.contains("world"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cast_output_is_durable_after_drop() {
        let dir = std::env::temp_dir().join(format!("termproof-cast-drop-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("drop.cast");
        {
            let mut rec = CastRecorder::new(path.clone(), 40, 10, vec!["sh".into()]).unwrap();
            rec.record_output("dropped\n");
            // Drop without explicit finish.
        }
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("dropped"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cast_empty_data_is_not_recorded() {
        let dir = std::env::temp_dir().join(format!("termproof-cast-empty-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.cast");
        let mut rec = CastRecorder::new(path.clone(), 80, 24, vec!["sh".into()]).unwrap();
        rec.record_output("");
        rec.record_input("");
        rec.finish().unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        // Only header.
        assert_eq!(content.lines().count(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
