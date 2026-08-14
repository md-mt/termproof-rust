//! Durations are read with a Python `float()` coercion, and only
//! `wait_for_regex` range-checks the result.
//!
//! `specs/002-builtin-steps/spec.md` FR-004: every duration-valued key —
//! `timeout_seconds` on three steps, `stable_seconds` on `wait_for_idle`,
//! `seconds` on `sleep` — "is read with a Python `float()` coercion. It
//! therefore accepts JSON numbers, numeric JSON strings (`"0.05"`), and
//! booleans". The port accepted only a JSON number, so a recipe the oracle runs
//! was rejected by the port before it started.
//!
//! FR-006 is the other half: `wait_for_text` and `wait_for_idle` "apply **no**
//! range validation". A NaN, zero or negative timeout is an instant timeout,
//! not a validation error, and a negative, NaN or infinite `stable_seconds`
//! passes on the first iteration. The spec is explicit that this is behaviour
//! the oracle *has* rather than behaviour it *should* have — recorded as
//! OQ-003 — and equally explicit that it is the contract until superseded.
//!
//! FR-005 is the exception the asymmetry rests on: `wait_for_regex`, and only
//! `wait_for_regex`, rejects a non-finite or non-positive timeout. That is
//! OQ-001, and it is not resolved here.
//!
//! JSON has no literal for NaN or infinity, so the non-finite cases arrive the
//! way a recipe would have to write them and the way the corpus records them:
//! as the strings `float()` maps to those values.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value as JsonValue};

use termproof_core::steps;
use termproof_terminal::InMemorySession;

/// `InMemorySession` answers a wait from its fixed content and ignores the
/// deadline, so a step that coerced `0.0` and a step that coerced `10.0` look
/// alike through it. What this file can hold the step layer to is that the
/// value was *accepted* and turned into a duration rather than refused; that a
/// zero deadline then exits the loop on its first test is the session's
/// contract, transcribed from the oracle and measured end to end by
/// `differential_steps.rs`.
fn was_not_rejected(detail: &str, key: &str) -> bool {
    !detail.contains(&format!("{key} must be"))
}

/// Anything past this is not waiting, it is stuck.
const PROMPT: Duration = Duration::from_secs(5);

fn run(screen: &str, step: JsonValue) -> (bool, String) {
    let mut s = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
    s.set_screen(screen.to_string());
    s.set_raw(screen.to_string());
    let started = Instant::now();
    let result = steps::dispatch(&mut s, &step, 1);
    assert!(started.elapsed() < PROMPT, "{step} did not return promptly");
    (result.passed, result.detail)
}

// ---- FR-004: what `float()` accepts ---------------------------------------

#[test]
fn a_numeric_string_is_a_duration_on_every_step() {
    let cases = [
        json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": "0.05"}),
        json!({"action": "wait_for_idle", "stable_seconds": "0.01", "timeout_seconds": "5"}),
        json!({"action": "sleep", "seconds": "0.01"}),
        json!({"action": "wait_for_regex", "pattern": "hello", "timeout_seconds": "0.05"}),
    ];
    for case in cases {
        let (passed, detail) = run("hello", case.clone());
        assert!(passed, "{case} was rejected: {detail}");
    }
}

#[test]
fn a_boolean_is_a_duration() {
    // `true` is 1.0 -- a real timeout.
    let (passed, detail) = run(
        "hello",
        json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": true}),
    );
    assert!(passed, "{detail}");

    // `false` is 0.0 -- an instant timeout, so the step runs and reports
    // whatever the wait reports. What it must not do is refuse the value.
    let (_, detail) = run(
        "hello",
        json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": false}),
    );
    assert!(
        was_not_rejected(&detail, "timeout_seconds"),
        "false should coerce to 0.0: {detail}"
    );

    // `sleep` for 0.0 seconds sleeps and passes -- no wait loop involved, so
    // this one is end-to-end.
    let (passed, detail) = run("", json!({"action": "sleep", "seconds": false}));
    assert!(passed, "{detail}");
}

#[test]
fn whitespace_around_a_numeric_string_is_ignored() {
    let (passed, detail) = run(
        "hello",
        json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": " 0.05 "}),
    );
    assert!(passed, "{detail}");
}

#[test]
fn a_value_float_rejects_still_fails_the_step() {
    for value in [json!("abc"), json!(null), json!([]), json!({})] {
        let (passed, detail) = run(
            "hello",
            json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": value}),
        );
        assert!(!passed, "timeout_seconds {value} should be rejected");
        assert!(detail.contains("timeout_seconds"), "{detail}");
    }
}

// ---- FR-006: no range validation on the two plain waits -------------------

#[test]
fn a_non_positive_timeout_is_accepted_rather_than_rejected() {
    for value in [json!(0), json!(-1), json!("nan")] {
        let (_, detail) = run(
            "hello",
            json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": value}),
        );
        assert!(
            was_not_rejected(&detail, "timeout_seconds"),
            "timeout_seconds {value} is an instant timeout, not an error: {detail}"
        );
    }
}

#[test]
fn a_non_positive_idle_timeout_is_accepted_rather_than_rejected() {
    for value in [json!(0), json!(-1), json!("nan")] {
        let (_, detail) = run(
            "",
            json!({"action": "wait_for_idle", "stable_seconds": 5, "timeout_seconds": value}),
        );
        assert!(
            was_not_rejected(&detail, "timeout_seconds"),
            "timeout_seconds {value} is an instant timeout, not an error: {detail}"
        );
    }
}

/// An infinite timeout waits, so a condition already satisfied passes at once.
#[test]
fn an_infinite_timeout_waits_rather_than_failing() {
    let (passed, detail) = run(
        "hello",
        json!({"action": "wait_for_text", "text": "hello", "timeout_seconds": "inf"}),
    );
    assert!(passed, "{detail}");
}

/// FR-006, and the detail is FR-009's float `repr` with an `s` glued on. The
/// spec calls these accidents (OQ-003) and contract in the same breath.
#[test]
fn a_negative_or_non_finite_stable_window_succeeds_immediately() {
    for (value, want) in [
        (json!(-1), "stable for -1.0s"),
        (json!("nan"), "stable for nans"),
        (json!("inf"), "stable for infs"),
        (json!("0.05"), "stable for 0.05s"),
    ] {
        let (passed, detail) = run(
            "x",
            json!({"action": "wait_for_idle", "stable_seconds": value, "timeout_seconds": 5}),
        );
        assert!(passed, "stable_seconds {value}: {detail}");
        assert_eq!(detail, want, "stable_seconds {value}");
    }
}

// ---- FR-005: the one step that does validate ------------------------------

/// The asymmetry is OQ-001 and stands until someone decides otherwise.
/// Widening the coercion must not quietly widen this too.
#[test]
fn wait_for_regex_still_rejects_what_it_always_rejected() {
    for (value, want) in [
        (json!("nan"), "must be finite, got nan"),
        (json!("inf"), "must be finite, got inf"),
        (json!("-inf"), "must be finite, got -inf"),
        (json!(0), "must be > 0, got 0.0"),
        (json!(false), "must be > 0, got 0.0"),
        (json!(-1), "must be > 0, got -1.0"),
    ] {
        let (passed, detail) = run(
            "hello",
            json!({"action": "wait_for_regex", "pattern": "hello", "timeout_seconds": value}),
        );
        assert!(!passed, "timeout_seconds {value} should be rejected");
        assert_eq!(detail, format!("wait_for_regex timeout_seconds {want}"));
    }
}

/// FR-005 renders the original value here, so the coercion must not have
/// consumed it: a numeric string that `float()` rejects reports the string.
#[test]
fn wait_for_regex_reports_the_value_it_was_given() {
    let (_, detail) = run(
        "hello",
        json!({"action": "wait_for_regex", "pattern": "hello", "timeout_seconds": "abc"}),
    );
    assert_eq!(
        detail,
        "wait_for_regex timeout_seconds must be a number, got 'abc'"
    );
}

// ---- FR-008: sleep validates nothing and rejects at the primitive ---------

#[test]
fn sleep_rejects_what_the_platform_clock_cannot_take() {
    for value in [json!("nan"), json!("inf"), json!("-inf"), json!(-1)] {
        let (passed, detail) = run("", json!({"action": "sleep", "seconds": value}));
        assert!(!passed, "sleep {value} should fail");
        assert!(!detail.contains('\n'), "{detail:?}");
    }
}
