//! Every value a `detail` embeds is rendered with Python's `repr`.
//!
//! Constitution Principle VIII, restated as FR-028 of
//! `specs/002-builtin-steps/spec.md`: no Rust `Debug` rendering appears in any
//! `detail`. The port used `{:?}` throughout, which is close enough to `repr`
//! to look right and differs in four ways that all reach the report — quoting,
//! the apostrophe flip, the `.0` on integral floats, and the tuple's trailing
//! comma.
//!
//! FR-020 is the requirement with the most `repr` in it, and the worked
//! examples below are the ones recorded from the oracle. FR-009 covers the
//! duration renderings; the review recorded the port emitting `10s` where the
//! oracle emits `10.0s`.

use std::path::PathBuf;

use serde_json::{json, Value as JsonValue};

use termproof_core::steps;
use termproof_terminal::InMemorySession;

fn detail(screen: &str, step: JsonValue) -> String {
    let mut s = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
    s.set_screen(screen.to_string());
    s.set_raw(screen.to_string());
    steps::dispatch(&mut s, &step, 1).detail
}

fn regex_detail(screen: &str, pattern: &str) -> String {
    detail(
        screen,
        json!({"action": "wait_for_regex", "pattern": pattern, "timeout_seconds": 0.05}),
    )
}

#[test]
fn found_text_is_single_quoted() {
    assert_eq!(
        detail("hello", json!({"action": "wait_for_text", "text": "hello"})),
        "found 'hello'"
    );
}

#[test]
fn text_that_was_not_found_is_single_quoted_too() {
    assert_eq!(
        detail(
            "hello",
            json!({"action": "wait_for_text", "text": "zzz", "timeout_seconds": 0.05})
        ),
        "timed out waiting for 'zzz'"
    );
}

/// Python's `repr` flips to double quotes when the value contains an
/// apostrophe and no double quote. Nothing in Rust does this.
#[test]
fn an_apostrophe_flips_the_quoting() {
    assert_eq!(
        detail(
            "it's here",
            json!({"action": "wait_for_text", "text": "it's"})
        ),
        "found \"it's\""
    );
}

/// FR-009. An integral duration keeps its point: `1.0s`, not `1s`.
#[test]
fn an_integral_duration_keeps_its_decimal_point() {
    assert_eq!(
        detail(
            "x",
            json!({"action": "wait_for_idle", "stable_seconds": 1, "timeout_seconds": 5})
        ),
        "stable for 1.0s"
    );
    assert_eq!(
        detail(
            "x",
            json!({"action": "wait_for_regex", "pattern": "zzz", "timeout_seconds": 1})
        ),
        "timed out waiting for regex 'zzz' after 1.0s"
    );
}

#[test]
fn a_fractional_duration_renders_shortest() {
    assert_eq!(
        detail(
            "x",
            json!({"action": "wait_for_idle", "stable_seconds": 0.05, "timeout_seconds": 5})
        ),
        "stable for 0.05s"
    );
}

#[test]
fn a_match_with_no_groups_reports_the_whole_match() {
    assert_eq!(
        regex_detail("abc 42", r"\d+"),
        r"matched '\\d+' -> match='42'"
    );
}

#[test]
fn positional_groups_render_as_a_python_tuple() {
    assert_eq!(
        regex_detail("alice 42", r"(\w+) (\d+)"),
        r"matched '(\\w+) (\\d+)' -> groups=('alice', '42') (full: 'alice 42')"
    );
}

/// FR-020 names this one: a one-element tuple keeps its trailing comma.
#[test]
fn a_one_element_group_tuple_keeps_its_trailing_comma() {
    assert_eq!(
        regex_detail("42", r"(?P<n>\d+)"),
        r"matched '(?P<n>\\d+)' -> n='42'; groups=('42',) (full: '42')"
    );
}

/// An unmatched group is part of the report, not absent from it. Dropping it
/// silently renumbers what the reader sees.
#[test]
fn unmatched_groups_render_as_none() {
    assert_eq!(
        regex_detail("b", "(?P<x>a)?(?P<y>b)"),
        "matched '(?P<x>a)?(?P<y>b)' -> x=None, y='b'; groups=(None, 'b') (full: 'b')"
    );
}

#[test]
fn an_apostrophe_in_a_match_flips_the_quoting() {
    assert_eq!(
        regex_detail("it's here", "(it's)"),
        "matched \"(it's)\" -> groups=(\"it's\",) (full: \"it's\")"
    );
}

#[test]
fn a_newline_in_a_match_is_escaped_not_embedded() {
    let d = regex_detail("h\nllo", "(?s)h.llo");
    assert_eq!(d, r"matched '(?s)h.llo' -> match='h\nllo'");
    assert!(!d.contains('\n'), "a detail must stay on one line: {d:?}");
}

#[test]
fn an_invalid_pattern_is_single_quoted() {
    assert!(
        regex_detail("x", "[bad").starts_with("invalid regex '[bad': "),
        "{}",
        regex_detail("x", "[bad")
    );
}

/// FR-005: the range messages render the coerced float, so `0` reads as `0.0`.
#[test]
fn a_rejected_duration_renders_as_a_float() {
    assert_eq!(
        detail(
            "x",
            json!({"action": "wait_for_regex", "pattern": "a", "timeout_seconds": 0})
        ),
        "wait_for_regex timeout_seconds must be > 0, got 0.0"
    );
    assert_eq!(
        detail(
            "x",
            json!({"action": "wait_for_regex", "pattern": "a", "timeout_seconds": -1})
        ),
        "wait_for_regex timeout_seconds must be > 0, got -1.0"
    );
}

/// FR-005: the wrong-type message renders the original value, not a float.
#[test]
fn a_wrong_typed_duration_renders_the_value_it_was_given() {
    for (value, want) in [
        (json!("abc"), "'abc'"),
        (json!(null), "None"),
        (json!([]), "[]"),
    ] {
        assert_eq!(
            detail(
                "x",
                json!({"action": "wait_for_regex", "pattern": "a", "timeout_seconds": value})
            ),
            format!("wait_for_regex timeout_seconds must be a number, got {want}")
        );
    }
}

/// FR-022: type names are Python's, so a JSON integer is an `int` and not a
/// `number`.
#[test]
fn type_names_are_pythons() {
    for (value, want) in [
        (json!(42), "int"),
        (json!(4.5), "float"),
        (json!(null), "NoneType"),
        (json!([]), "list"),
        (json!(true), "bool"),
    ] {
        assert_eq!(
            detail("x", json!({"action": "wait_for_regex", "pattern": value})),
            format!("wait_for_regex 'pattern' must be a string, got {want}")
        );
    }
}

/// A missing `pattern` is indistinguishable from an explicit `null` — FR-018.
#[test]
fn a_missing_pattern_reports_the_none_type() {
    assert_eq!(
        detail("x", json!({"action": "wait_for_regex"})),
        "wait_for_regex 'pattern' must be a string, got NoneType"
    );
}

/// FR-028: no `detail` anywhere carries Rust's `Debug` rendering.
#[test]
fn no_detail_carries_a_rust_debug_rendering() {
    let cases = [
        json!({"action": "wait_for_text", "text": "hello"}),
        json!({"action": "wait_for_regex", "pattern": 42}),
        json!({"action": "send_line", "text": 5}),
        json!({"action": "press", "key": "f13"}),
        json!({"action": "sleep", "seconds": -1}),
    ];
    for case in cases {
        let d = detail("hello", case.clone());
        for forbidden in ["Some(", "None)", "Object(", "String(", "Number("] {
            assert!(
                !d.contains(forbidden),
                "{case} produced a Rust Debug rendering: {d:?}"
            );
        }
        assert!(
            !d.contains('\n'),
            "{case} produced a multi-line detail: {d:?}"
        );
    }
}
