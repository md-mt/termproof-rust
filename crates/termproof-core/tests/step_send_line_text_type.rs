//! `send_line` must not report success for input it never sent.
//!
//! `specs/002-builtin-steps/spec.md` FR-013: `send_line` reads `text`,
//! defaulting to `""`, and writes `text + "\r"`. FR-022 records what the oracle
//! does when `text` is present but not a string — `"text": 5` and
//! `"text": null` both fail the step.
//!
//! The port read `text` with `as_str().unwrap_or("")`, which cannot tell a
//! missing key from a wrong-typed one. Every non-string `text` sent a bare `\r`
//! and reported `sent line`. That is the worst shape a defect can take in a
//! verification tool: the recipe is wrong, nothing was sent, and the report
//! says it passed.
//!
//! FR-025 sets the other half of the boundary — a wrong type is a failed step,
//! never a terminated run.

use std::path::PathBuf;

use serde_json::json;

use termproof_core::steps;
use termproof_terminal::{InMemorySession, Session};

fn session() -> InMemorySession {
    InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24)
}

/// FR-013: a missing `text` is a default, not an error.
#[test]
fn a_missing_text_sends_the_bare_carriage_return() {
    let mut s = session();
    let result = steps::dispatch(&mut s, &json!({"action": "send_line"}), 1);
    assert!(result.passed, "detail was {:?}", result.detail);
    assert_eq!(result.detail, "sent line");
    assert!(
        s.log.iter().any(|e| e == "send_line:"),
        "log was {:?}",
        s.log
    );
}

#[test]
fn an_empty_text_still_passes() {
    let mut s = session();
    let result = steps::dispatch(&mut s, &json!({"action": "send_line", "text": ""}), 1);
    assert!(result.passed, "detail was {:?}", result.detail);
}

/// FR-022: each of these fails against the oracle. The port passed all four.
#[test]
fn a_non_string_text_fails_rather_than_sending_nothing() {
    for text in [json!(5), json!(null), json!(true), json!([]), json!({})] {
        let mut s = session();
        let result = steps::dispatch(
            &mut s,
            &json!({"action": "send_line", "text": text.clone()}),
            1,
        );
        assert!(
            !result.passed,
            "send_line text: {text} reported success: {:?}",
            result.detail
        );
        assert!(
            !result.detail.is_empty(),
            "send_line text: {text} failed without saying why"
        );
    }
}

/// A step that did not send must not leave the session thinking it did.
#[test]
fn a_rejected_text_writes_nothing_to_the_child() {
    let mut s = session();
    let _ = steps::dispatch(&mut s, &json!({"action": "send_line", "text": 5}), 1);
    assert!(s.log.is_empty(), "log was {:?}", s.log);
}

/// FR-025: a wrong type fails the step; it never ends the run.
#[test]
fn a_rejected_text_does_not_stop_later_steps() {
    let mut s = session();
    s.set_screen("ready".to_string());
    s.set_raw("ready".to_string());
    let results = steps::run_steps(
        &mut s,
        &[
            json!({"action": "send_line", "text": 5}),
            json!({"action": "wait_for_text", "text": "ready"}),
        ],
    );
    assert!(!results[0].passed);
    assert!(s.is_alive(), "the session must survive a wrong-typed input");
}
