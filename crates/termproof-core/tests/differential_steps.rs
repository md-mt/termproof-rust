//! The port half of the cross-runtime differential harness.
//!
//! Replays `harness/corpus/cases.json` — whose expectations were recorded from
//! the Python implementation by `harness/probe_steps.py` — through the Rust
//! steps, and reports how many cases agree. See `harness/README.md` for what
//! the corpus does and does not measure, and for the three deliberate
//! compromises in its construction.
//!
//! Each case runs on its own thread behind a wall-clock budget, so a case that
//! panics or never returns is recorded as a divergence instead of taking the
//! whole measurement down with it. Both are failure modes the port has.
//!
//! Run it with output:
//!
//! ```sh
//! cargo test -p termproof-core --test differential_steps -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value as JsonValue};

use termproof_core::result::StepResult;
use termproof_core::steps;
use termproof_terminal::{InMemorySession, Session, SessionError};

/// The measurement, locked in as a ratchet. Raising the floor and lowering the
/// ceilings is the point of the harness; moving either the other way is a
/// regression, and needs saying out loud in the change that does it.
///
/// At the commit that introduced this file: 26 / 115, five panics and one case
/// that never returned.
const AGREEMENT_FLOOR: usize = 82;
/// Cases agreeing on pass/fail, whatever the detail says. A fix that corrects a
/// verdict but leaves the wording to a later commit moves this and not the
/// floor above, so it still has to move a number.
const VERDICT_FLOOR: usize = 113;

/// Wall-clock budget per case. The slowest legitimate case in the corpus waits
/// three seconds; anything past this is not waiting, it is stuck.
const CASE_BUDGET: Duration = Duration::from_secs(10);

/// Populated by the panic hook so a panicking case can be reported with its
/// message rather than as an anonymous death. Cases run one at a time.
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Session with fixed content whose wait loops are transcribed from
/// `termproof/session.py`, so the corpus measures the step layer rather than
/// terminal fidelity. Everything else delegates to the real in-memory session,
/// so `press` and `send_*` exercise the port's own behaviour.
struct HarnessSession {
    inner: InMemorySession,
}

impl HarnessSession {
    fn new(screen: &str, raw: &str, alive: bool) -> Self {
        let mut inner = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
        inner.set_screen(screen.to_string());
        inner.set_raw(raw.to_string());
        inner.set_alive(alive);
        Self { inner }
    }

    fn holds(&self, text: &str) -> bool {
        self.inner.screen().contains(text) || self.inner.raw_output().contains(text)
    }
}

/// `Instant + Duration` panics on overflow. The harness must never be the thing
/// that panics, or the measurement stops being about the code under test.
fn deadline_from(timeout: Duration) -> Option<Instant> {
    Instant::now().checked_add(timeout)
}

fn before(deadline: Option<Instant>) -> bool {
    match deadline {
        Some(d) => Instant::now() < d,
        None => true,
    }
}

impl Session for HarnessSession {
    fn wait_for_text(&mut self, text: &str, timeout: Duration) -> Result<bool, SessionError> {
        // while time.monotonic() < deadline: ... if not is_alive(): return <found>
        let deadline = deadline_from(timeout);
        while before(deadline) {
            if self.holds(text) {
                return Ok(true);
            }
            if !self.inner.is_alive() {
                return Ok(self.holds(text));
            }
        }
        Ok(false)
    }

    fn wait_for_idle(&mut self, stable: Duration, timeout: Duration) -> Result<bool, SessionError> {
        // The content never changes, so the stable window arms once at entry if
        // any raw output exists and never re-arms.
        let deadline = deadline_from(timeout);
        let armed = !self.inner.raw_output().is_empty();
        let stable_since = Instant::now();
        while before(deadline) {
            if armed && stable_since.elapsed() >= stable {
                return Ok(true);
            }
            if !self.inner.is_alive() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn send_text(&mut self, text: &str) -> Result<(), SessionError> {
        self.inner.send_text(text)
    }
    fn send_line(&mut self, text: &str) -> Result<(), SessionError> {
        self.inner.send_line(text)
    }
    fn press(&mut self, key: &str) -> Result<(), SessionError> {
        self.inner.press(key)
    }
    fn wait_for_exit(&mut self, timeout: Duration) -> Result<Option<i32>, SessionError> {
        self.inner.wait_for_exit(timeout)
    }
    fn read_available(&mut self, timeout: Duration) -> Result<(), SessionError> {
        self.inner.read_available(timeout)
    }
    fn is_alive(&mut self) -> bool {
        self.inner.is_alive()
    }
    fn close(&mut self) -> Result<(), SessionError> {
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
    fn cast_path(&self) -> &Path {
        self.inner.cast_path()
    }
}

/// JSON cannot hold `NaN` or `Infinity`, so the corpus spells them as sentinel
/// strings. Substitute the spelling this runtime's duration coercion maps to
/// the same float — Python's `float()` and the port's coercion both read
/// `"nan"`, `"inf"` and `"-inf"`.
fn expand_sentinels(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::String(s) => match s.as_str() {
            "@nan" => json!("nan"),
            "@inf" => json!("inf"),
            "@-inf" => json!("-inf"),
            _ => value.clone(),
        },
        JsonValue::Object(map) => JsonValue::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), expand_sentinels(v)))
                .collect(),
        ),
        JsonValue::Array(items) => JsonValue::Array(items.iter().map(expand_sentinels).collect()),
        _ => value.clone(),
    }
}

/// What the port did with one case.
enum Outcome {
    Returned(Box<StepResult>),
    Panicked(String),
    Stuck,
}

fn run_case(step: JsonValue, screen: String, raw: String, alive: bool, index: usize) -> Outcome {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().spawn(move || {
        let mut session = HarnessSession::new(&screen, &raw, alive);
        let _ = tx.send(steps::dispatch(&mut session, &step, index));
    });
    // A thread that cannot be spawned is an environment failure, not a finding.
    spawned.expect("spawn case thread");

    match rx.recv_timeout(CASE_BUDGET) {
        Ok(result) => Outcome::Returned(Box::new(result)),
        // The sender is dropped without sending only when the case panicked.
        Err(RecvTimeoutError::Disconnected) => Outcome::Panicked(
            LAST_PANIC
                .lock()
                .expect("panic slot")
                .take()
                .unwrap_or_else(|| "panicked".to_string()),
        ),
        // The thread is left running: it is spinning on a deadline it will not
        // reach, and there is no way to interrupt it. It dies with the process.
        Err(RecvTimeoutError::Timeout) => Outcome::Stuck,
    }
}

fn corpus_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/termproof-core.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/corpus/steps.expected.json")
}

#[test]
fn differential_steps_against_python() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panicked".to_string());
        *LAST_PANIC.lock().expect("panic slot") = Some(message);
    }));

    let raw = std::fs::read_to_string(corpus_path()).expect("read recorded corpus");
    let document: JsonValue = serde_json::from_str(&raw).expect("parse recorded corpus");
    let cases = document["cases"].as_array().expect("corpus cases array");

    let mut agreed = 0usize;
    let mut verdict_only = 0usize;
    let mut panicked = 0usize;
    let mut stuck = 0usize;
    let mut divergences: Vec<String> = Vec::new();

    for case in cases {
        let id = case["id"].as_str().unwrap_or("<unnamed>");
        let index = case["index"].as_u64().unwrap_or(1) as usize;
        let step = expand_sentinels(&case["step"]);
        let session_spec = &case["session"];
        let screen = session_spec["screen"].as_str().unwrap_or("").to_string();
        let raw_output = session_spec["raw"].as_str().unwrap_or("").to_string();
        let alive = session_spec["alive"].as_bool().unwrap_or(true);

        let expected = &case["expected"];
        let want_name = expected["name"].as_str().unwrap_or("");
        let want_passed = expected["passed"].as_bool().unwrap_or(false);
        let want_detail = expected["detail"].as_str().unwrap_or("");

        let result = match run_case(step, screen, raw_output, alive, index) {
            Outcome::Returned(result) => result,
            Outcome::Panicked(message) => {
                panicked += 1;
                divergences.push(format!(
                    "{id}:\n    python: {want_passed} {want_detail:?}\n    rust:   PANIC {message:?}"
                ));
                continue;
            }
            Outcome::Stuck => {
                stuck += 1;
                divergences.push(format!(
                    "{id}:\n    python: {want_passed} {want_detail:?}\n    rust:   did not return within {CASE_BUDGET:?}"
                ));
                continue;
            }
        };

        if result.name == want_name && result.passed == want_passed && result.detail == want_detail
        {
            agreed += 1;
            continue;
        }
        if result.passed == want_passed {
            verdict_only += 1;
        }
        divergences.push(format!(
            "{id}:\n    python: {} {:?}\n    rust:   {} {:?}",
            want_passed, want_detail, result.passed, result.detail
        ));
    }

    let _ = std::panic::take_hook();

    let total = cases.len();
    println!("\n=== differential harness: steps ===");
    println!("oracle environment: {}", document["environment"]);
    for line in &divergences {
        println!("  {line}");
    }
    println!("\nfull agreement (name + passed + detail): {agreed}/{total}");
    println!("verdict agrees, detail differs:          {verdict_only}/{total}");
    println!("panics:                                  {panicked}/{total}");
    println!("did not return:                          {stuck}/{total}");
    println!(
        "passed/failed verdict agreement:         {}/{total}",
        agreed + verdict_only
    );

    // Recipe-controlled input must never take the process down or wedge the
    // run. Both were non-zero when this file was written; neither may come back.
    assert_eq!(panicked, 0, "{panicked} cases panicked");
    assert_eq!(stuck, 0, "{stuck} cases never returned");
    assert!(
        agreed >= AGREEMENT_FLOOR,
        "agreement dropped to {agreed}/{total}, below the recorded floor of {AGREEMENT_FLOOR}"
    );
    assert!(
        agreed + verdict_only >= VERDICT_FLOOR,
        "verdict agreement dropped to {}/{total}, below the recorded floor of {VERDICT_FLOOR}",
        agreed + verdict_only
    );
}
