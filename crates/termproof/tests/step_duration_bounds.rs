//! A large finite duration is legal input, and must not take the process down.
//!
//! `specs/002-builtin-steps/spec.md` FR-007: the oracle accepts `1e19` and
//! `1e300` on every waiting step and simply waits. A Rust implementation "MUST
//! NOT panic, saturate silently, or reject the value; the correct behaviour is
//! to clamp the internal deadline to the far future and keep waiting."
//!
//! `Duration::from_secs_f64` panics above roughly `1.8e19`, and
//! `Instant::now() + d` panics well below that. Both are reachable from a
//! recipe, and a validator that dies on a bad recipe cannot report what went
//! wrong.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;

use termproof::steps;
use termproof::terminal::InMemorySession;

/// Anything past this is not waiting, it is stuck. Every case here either
/// returns immediately or is a bug.
const PROMPT: Duration = Duration::from_secs(5);

fn session(screen: &str) -> InMemorySession {
    let mut s = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
    s.set_screen(screen.to_string());
    s.set_raw(screen.to_string());
    s
}

#[test]
fn wait_for_text_accepts_a_duration_beyond_duration_from_secs_f64() {
    let mut s = session("hello world");
    let started = Instant::now();
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": 1e300}),
        1,
    );
    assert!(started.elapsed() < PROMPT, "did not return promptly");
    assert!(result.passed, "detail was {:?}", result.detail);
}

#[test]
fn wait_for_idle_accepts_a_duration_beyond_duration_from_secs_f64() {
    let mut s = session("x");
    let started = Instant::now();
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_idle", "stable_seconds": 0.05, "timeout_seconds": 1e300}),
        1,
    );
    assert!(started.elapsed() < PROMPT, "did not return promptly");
    assert!(result.passed, "detail was {:?}", result.detail);
}

#[test]
fn wait_for_regex_accepts_a_deadline_beyond_instant_add() {
    let mut s = session("abc 42");
    let started = Instant::now();
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_regex", "pattern": "\\d+", "timeout_seconds": 1e19}),
        1,
    );
    assert!(started.elapsed() < PROMPT, "did not return promptly");
    assert!(result.passed, "detail was {:?}", result.detail);
}

#[test]
fn wait_for_regex_accepts_a_duration_beyond_duration_from_secs_f64() {
    let mut s = session("abc 42");
    let started = Instant::now();
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_regex", "pattern": "\\d+", "timeout_seconds": 1e300}),
        1,
    );
    assert!(started.elapsed() < PROMPT, "did not return promptly");
    assert!(result.passed, "detail was {:?}", result.detail);
}

/// `sleep` is the one duration that cannot be clamped: clamping it would hang
/// the run for a century. The oracle's own sleep primitive rejects a value out
/// of `time_t` range, so rejecting is the parity-preserving answer -- but it
/// must be a failed step, not a panic and not a wait.
#[test]
fn sleep_rejects_a_duration_beyond_the_platform_clock_without_panicking() {
    for seconds in [1e19, 1e300] {
        let mut s = session("");
        let started = Instant::now();
        let result = steps::dispatch(&mut s, &json!({"action": "sleep", "seconds": seconds}), 1);
        assert!(
            started.elapsed() < PROMPT,
            "sleep {seconds} did not return promptly"
        );
        assert!(!result.passed, "sleep {seconds} should fail");
        assert!(
            !result.detail.contains('\n'),
            "detail must stay on one line: {:?}",
            result.detail
        );
    }
}

/// The clamp must not change what an ordinary timeout does.
#[test]
fn an_ordinary_timeout_still_times_out() {
    let mut s = session("hello");
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_regex", "pattern": "zzz", "timeout_seconds": 0.05}),
        1,
    );
    assert!(!result.passed, "detail was {:?}", result.detail);
}
