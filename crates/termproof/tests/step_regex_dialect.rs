//! `wait_for_regex` speaks Python 3's `re` dialect, not the `regex` crate's.
//!
//! `specs/002-builtin-steps/spec.md` FR-019 tabulates thirteen patterns that
//! were executed against the oracle and MUST hold. Three of them the `regex`
//! crate cannot express at all — it has no lookaround and no backreferences —
//! and three more it disagrees with silently, which is worse: the recipe
//! compiles and quietly means something else.
//!
//! Every case below is a row of that table.

use std::path::PathBuf;

use serde_json::json;

use termproof::steps;
use termproof::terminal::InMemorySession;

fn matches(pattern: &str, haystack: &str) -> (bool, String) {
    let mut s = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
    s.set_screen(haystack.to_string());
    s.set_raw(haystack.to_string());
    let result = steps::dispatch(
        &mut s,
        &json!({"action": "wait_for_regex", "pattern": pattern, "timeout_seconds": 0.05}),
        1,
    );
    (result.passed, result.detail)
}

fn assert_matches(pattern: &str, haystack: &str) {
    let (passed, detail) = matches(pattern, haystack);
    assert!(passed, "{pattern:?} should match {haystack:?}: {detail}");
}

fn assert_does_not_match(pattern: &str, haystack: &str) {
    let (passed, detail) = matches(pattern, haystack);
    assert!(
        !passed,
        "{pattern:?} should not match {haystack:?}: {detail}"
    );
    assert!(
        detail.starts_with("timed out"),
        "{pattern:?} should compile and then not match, not fail to compile: {detail}"
    );
}

fn assert_rejected(pattern: &str) {
    let (passed, detail) = matches(pattern, "a");
    assert!(!passed, "{pattern:?} should be rejected: {detail}");
    assert!(
        detail.starts_with("invalid regex"),
        "{pattern:?} should be reported as an invalid pattern: {detail}"
    );
    // Constitution Principle VIII: the `regex` crate's multi-line ASCII-art
    // parse errors are forbidden outright.
    assert!(
        !detail.contains('\n'),
        "an invalid-pattern detail must stay on one line: {detail:?}"
    );
}

#[test]
fn lookbehind_matches() {
    assert_matches("(?<=a)b", "ab");
}

#[test]
fn backreference_matches() {
    assert_matches("(a)\\1", "aa");
}

#[test]
fn inline_case_flag_matches() {
    assert_matches("(?i)ALICE", "alice");
}

#[test]
fn inline_multiline_matches() {
    assert_matches("(?m)^world", "hello\nworld");
}

#[test]
fn caret_defaults_to_string_start() {
    assert_matches("^hello", "hello\nworld");
    assert_does_not_match("^world", "hello\nworld");
}

#[test]
fn dot_excludes_newline_by_default() {
    assert_does_not_match("h.llo", "h\nllo");
}

#[test]
fn inline_dotall_matches() {
    assert_matches("(?s)h.llo", "h\nllo");
}

#[test]
fn inline_comment_matches() {
    assert_matches("(?#comment)a", "a");
}

#[test]
fn python_named_group_spelling_matches() {
    assert_matches("(?P<n>\\d+)", "42");
}

#[test]
fn string_start_anchor_matches() {
    assert_matches("\\A\\w+", "alice");
}

/// `\Z` is Python's true end-of-string. A backtracking engine with PCRE
/// heritage reads it as "before a final newline", which makes `x\Z` match
/// `"x\n"` — a silent dialect difference, not a compile error.
#[test]
fn string_end_anchor_is_absolute() {
    assert_does_not_match("\\Zx", "x");
    assert_does_not_match("x\\Z", "x\n");
    assert_matches("x\\Z", "x");
}

#[test]
fn non_capturing_group_matches() {
    assert_matches("(?:a|b)+", "ab");
}

/// Python's `re` has no `\p{...}`; it raises rather than matching. An engine
/// that accepts it makes a recipe mean something the oracle would refuse.
#[test]
fn unicode_property_escape_is_rejected() {
    assert_rejected("\\p{L}");
    assert_rejected("\\P{L}");
}

/// Python raises on an inverted repeat. An engine that quietly reads
/// `a{2,1}` as `a{1,2}` turns a typo into a passing test.
#[test]
fn inverted_repetition_range_is_rejected() {
    assert_rejected("a{2,1}");
}

#[test]
fn malformed_patterns_are_rejected_on_one_line() {
    assert_rejected("[bad");
}

/// A `\p` inside a character class is the same escape and the same refusal.
#[test]
fn unicode_property_escape_is_rejected_inside_a_class() {
    assert_rejected("[\\p{L}]");
}

/// A brace that is not a quantifier is a literal in Python, and stays one.
#[test]
fn a_literal_brace_is_not_a_repetition_range() {
    assert_matches("a{", "a{");
    assert_matches("[{2,1}]", "{");
}
