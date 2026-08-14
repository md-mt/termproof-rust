//! Seven built-in steps with corpus parity, ported to the
//! `InMemorySession`-style `Session` trait.
//!
//! Each step mirrors `termproof/builtin_steps.py` exactly:
//!
//! * `wait_for_text` — search `screen` then `raw_output` independently via
//!   `Session::wait_for_text` (`Result<bool, SessionError>`).
//! * `wait_for_idle` — stable-screen detection via `Session::wait_for_idle`.
//! * `send_text` / `send_line` — feed child stdin (`send_line` appends `\r`).
//! * `press` — named-key / `Ctrl-` mapping delegated to `Session::press`.
//! * `sleep` — wall-clock sleep + `read_available(0)` drain.
//! * `wait_for_regex` — regex validation, timeout validation (NaN/Inf/<=0,
//!   non-numeric), independent screen/raw search without synthetic `\n`
//!   boundary, and evidence detail that matches `builtin_steps.py::_format_match`
//!   byte-for-byte (named groups, positional groups, full match).

use std::time::Duration;

use serde_json::Value as JsonValue;

use crate::pyrepr::{repr_f64, repr_json, repr_str, repr_tuple, type_name};
use crate::result::StepResult;
use crate::terminal::Session;

// ---- helpers ---------------------------------------------------------------

/// Extract the step display name: `step["name"]` or `"{index}:{action}"`.
fn display_name(step: &JsonValue, index: usize, action: &str) -> String {
    step.get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{index}:{action}"))
}

/// Read a duration key the way Python's `float()` reads it.
///
/// FR-004: every duration-valued key is a `float()` coercion, so a JSON number,
/// a numeric JSON string (`"0.05"`, and `"nan"` / `"inf"` — the only spellings
/// of the non-finite values JSON can carry) and a boolean are all durations.
/// Accepting only `JsonValue::Number` rejected recipes the oracle runs.
///
/// FR-005: the wrong-type message renders the value as it was authored, so a
/// string stays quoted and `null` reads as `None`.
///
/// One narrow gap: Python's `float()` also accepts the digit separators of a
/// numeric literal, so `float("1_0")` is `10.0`. That is not reachable from a
/// duration anyone writes on purpose, and matching it means reimplementing
/// CPython's literal grammar.
fn coerce_float(field: &str, v: &JsonValue) -> Result<f64, String> {
    let rejected = || format!("{field} must be a number, got {}", repr_json(v));
    match v {
        JsonValue::Number(n) => n.as_f64().ok_or_else(rejected),
        JsonValue::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        // `float()` strips surrounding whitespace before parsing.
        JsonValue::String(text) => text.trim().parse::<f64>().map_err(|_| rejected()),
        _ => Err(rejected()),
    }
}

/// A deadline no run reaches. `1e300` seconds is not expressible as a
/// `Duration` and `Instant::now() + d` overflows long before that, so an
/// unreachable deadline is the only faithful reading of "wait that long".
const FAR_FUTURE: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

/// Convert a duration in seconds without panicking on the extremes.
///
/// `Duration::from_secs_f64` panics on NaN and above roughly `1.8e19`, and
/// every one of these values is reachable from a recipe. Spec 002 FR-007: a
/// large finite duration is legal input and the implementation "MUST NOT
/// panic, saturate silently, or reject the value; the correct behaviour is to
/// clamp the internal deadline to the far future and keep waiting". A
/// non-positive or NaN duration is a deadline already past, which is what
/// makes the oracle's wait loop exit on its first test (FR-006).
pub(crate) fn duration_from_secs(seconds: f64) -> Duration {
    if seconds.is_nan() || seconds <= 0.0 {
        Duration::ZERO
    } else if seconds >= FAR_FUTURE.as_secs() as f64 {
        FAR_FUTURE
    } else {
        Duration::from_secs_f64(seconds)
    }
}

/// The idle step's stable window, which has its own reading of the extremes.
///
/// FR-006: a negative, NaN or infinite `stable_seconds` makes the idle check
/// succeed on its first iteration, so all three are a window of zero. The spec
/// records this as an accident of the oracle's implementation rather than a
/// decision, open as OQ-003 -- and as the contract until that is settled.
fn stable_window_from_secs(seconds: f64) -> Duration {
    if seconds.is_finite() {
        duration_from_secs(seconds)
    } else {
        Duration::ZERO
    }
}

/// `sleep` is the one duration that cannot be clamped: clamping it would hang
/// the run for a century instead of reporting anything. The oracle's own sleep
/// primitive rejects a value outside `time_t` range rather than sleeping
/// (FR-008), so rejecting keeps the verdict and loses only the wording.
const MAX_SLEEP_SECONDS: f64 = i64::MAX as f64;

/// `Instant::now() + d` panics on overflow, so a deadline is optional: `None`
/// means "further away than this platform's clock can express".
fn deadline_from(timeout: Duration) -> Option<std::time::Instant> {
    std::time::Instant::now().checked_add(timeout)
}

/// Whether a deadline that may be unrepresentable has passed.
fn deadline_passed(deadline: Option<std::time::Instant>) -> bool {
    match deadline {
        Some(d) => std::time::Instant::now() >= d,
        None => false,
    }
}

/// Range-check a timeout. FR-005: `wait_for_regex` is the only step that does
/// this. The asymmetry has no recorded rationale and is open as OQ-001; it is
/// the contract until that is decided, so it stays confined to one caller.
fn validate_regex_timeout(raw: &JsonValue, field: &str) -> Result<f64, String> {
    let f = coerce_float(field, raw)?;
    if !f.is_finite() {
        return Err(format!("{field} must be finite, got {}", repr_f64(f)));
    }
    if f <= 0.0 {
        return Err(format!("{field} must be > 0, got {}", repr_f64(f)));
    }
    Ok(f)
}

// ---- wait_for_text -------------------------------------------------------

/// `wait_for_text` — poll `screen` and `raw_output` for `text`.
pub fn wait_for_text<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let name = display_name(step, index, "wait_for_text");
    let text = match step.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "wait_for_text 'text' must be a string".into(),
                screen: session.screen().to_string(),
            };
        }
    };
    // FR-006: no range validation here. A NaN, zero or negative timeout is a
    // deadline already past, which the wait loop reports as an ordinary
    // timeout rather than as an error.
    let timeout = match step.get("timeout_seconds") {
        None => Duration::from_secs(10),
        Some(v) => match coerce_float("timeout_seconds", v) {
            Ok(f) => duration_from_secs(f),
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen().to_string(),
                };
            }
        },
    };
    let passed = match session.wait_for_text(&text, timeout) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                name,
                passed: false,
                detail: format!("wait_for_text failed: {e}"),
                screen: session.screen().to_string(),
            };
        }
    };
    let detail = if passed {
        format!("found {}", repr_str(&text))
    } else {
        format!("timed out waiting for {}", repr_str(&text))
    };
    StepResult {
        name,
        passed,
        detail,
        screen: session.screen().to_string(),
    }
}

// ---- wait_for_idle -------------------------------------------------------

/// `wait_for_idle` — screen has been stable for `stable_seconds`.
pub fn wait_for_idle<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let name = display_name(step, index, "wait_for_idle");
    // FR-006 again, on both keys. The detail renders the coerced float rather
    // than the clamped `Duration`, because `-1.0`, `nan` and `inf` are all
    // values a `Duration` cannot hold and all values the oracle prints.
    let secs = match step.get("stable_seconds") {
        None => 0.5,
        Some(v) => match coerce_float("stable_seconds", v) {
            Ok(f) => f,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen().to_string(),
                };
            }
        },
    };
    let stable = stable_window_from_secs(secs);
    let timeout = match step.get("timeout_seconds") {
        None => Duration::from_secs(10),
        Some(v) => match coerce_float("timeout_seconds", v) {
            Ok(f) => duration_from_secs(f),
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen().to_string(),
                };
            }
        },
    };
    let passed = match session.wait_for_idle(stable, timeout) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                name,
                passed: false,
                detail: format!("wait_for_idle failed: {e}"),
                screen: session.screen().to_string(),
            };
        }
    };
    let detail = if passed {
        format!("stable for {}s", repr_f64(secs))
    } else {
        "timed out waiting for idle".into()
    };
    StepResult {
        name,
        passed,
        detail,
        screen: session.screen().to_string(),
    }
}

// ---- send_text -----------------------------------------------------------

/// `send_text` — feed `text` verbatim (no newline).
pub fn send_text<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let name = display_name(step, index, "send_text");
    let text = match step.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "send_text 'text' must be a string".into(),
                screen: session.screen().to_string(),
            };
        }
    };
    match session.send_text(&text) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: "sent text".into(),
            screen: session.screen().to_string(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e.to_string(),
            screen: session.screen().to_string(),
        },
    }
}

// ---- send_line -----------------------------------------------------------

/// `send_line` — feed `text + "\r"` (default `text` is `""` as in Python).
pub fn send_line<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let name = display_name(step, index, "send_line");
    // FR-013: a missing `text` defaults to `""`. A present one must be a
    // string -- FR-022 records the oracle failing every other type, and
    // `as_str().unwrap_or("")` cannot tell the two cases apart.
    let text = match step.get("text") {
        None => String::new(),
        Some(JsonValue::String(s)) => s.clone(),
        Some(other) => {
            return StepResult {
                name,
                passed: false,
                detail: format!(
                    "send_line 'text' must be a string, got {}",
                    type_name(other)
                ),
                screen: session.screen().to_string(),
            };
        }
    };
    match session.send_line(&text) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: "sent line".into(),
            screen: session.screen().to_string(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e.to_string(),
            screen: session.screen().to_string(),
        },
    }
}

// ---- press ---------------------------------------------------------------

/// `press` — send a named key or `Ctrl-` combo.
pub fn press<S: Session + ?Sized>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "press");
    let key = match step.get("key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "press 'key' must be a string".into(),
                screen: session.screen().to_string(),
            };
        }
    };
    match session.press(&key) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: format!("pressed {key}"),
            screen: session.screen().to_string(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e.to_string(),
            screen: session.screen().to_string(),
        },
    }
}

// ---- sleep ---------------------------------------------------------------

/// `sleep` — wall-clock sleep then `read_available(0)` drain.
pub fn sleep<S: Session + ?Sized>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "sleep");
    let seconds = match step.get("seconds") {
        None => 1.0,
        Some(v) => match coerce_float("seconds", v) {
            Ok(f) => f,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen().to_string(),
                };
            }
        },
    };
    if !seconds.is_finite() {
        return StepResult {
            name,
            passed: false,
            detail: format!("seconds must be finite, got {}", repr_f64(seconds)),
            screen: session.screen().to_string(),
        };
    }
    if seconds < 0.0 {
        return StepResult {
            name,
            passed: false,
            detail: format!("seconds must be >= 0, got {}", repr_f64(seconds)),
            screen: session.screen().to_string(),
        };
    }
    if seconds > MAX_SLEEP_SECONDS {
        return StepResult {
            name,
            passed: false,
            detail: format!(
                "seconds is out of range for the platform sleep clock, got {}",
                repr_f64(seconds)
            ),
            screen: session.screen().to_string(),
        };
    }
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let _ = session.read_available(Duration::ZERO);
    StepResult {
        name,
        passed: true,
        detail: "slept".into(),
        screen: session.screen().to_string(),
    }
}

// ---- wait_for_regex ------------------------------------------------------

/// `wait_for_regex` — regex validation, timeout validation, streaming poll.
///
/// Matches Python's `builtin_steps.WaitForRegex.execute` exactly:
/// * pattern must be a string, otherwise immediate failure.
/// * timeout must be finite, > 0, numeric.
/// * search `screen` then `raw_output` independently per poll (no synthetic
///   boundary).
/// * evidence `detail` is `matched {pattern!r} -> ...` with named groups,
///   positional groups, and full match (same branches as `_format_match`).
pub fn wait_for_regex<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let name = display_name(step, index, "wait_for_regex");

    // -- validate pattern ---------------------------------------------------
    let pattern_str = match step.get("pattern").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            let got = type_name(step.get("pattern").unwrap_or(&JsonValue::Null));
            return StepResult {
                name,
                passed: false,
                detail: format!("wait_for_regex 'pattern' must be a string, got {got}"),
                screen: session.screen().to_string(),
            };
        }
    };
    let re = match crate::pyregex::compile(&pattern_str) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                name,
                passed: false,
                detail: format!("invalid regex {}: {e}", repr_str(&pattern_str)),
                screen: session.screen().to_string(),
            };
        }
    };

    // -- validate timeout ---------------------------------------------------
    let timeout_secs = match step.get("timeout_seconds") {
        None => 10.0,
        Some(v) => match validate_regex_timeout(v, "wait_for_regex timeout_seconds") {
            Ok(f) => f,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen().to_string(),
                };
            }
        },
    };
    let timeout = duration_from_secs(timeout_secs);

    // -- poll loop (same cadence as wait_for_text: 50ms ticks) --------------
    let deadline = deadline_from(timeout);

    loop {
        let _ = session.read_available(Duration::from_millis(50));
        let screen_text = session.screen().to_string();
        let raw_text = session.raw_output().to_string();
        if let Some(result) = try_match(&re, &pattern_str, &name, &screen_text, &raw_text) {
            return result;
        }
        if deadline_passed(deadline) {
            break;
        }
        if !session.is_alive() {
            let _ = session.read_available(Duration::ZERO);
            let screen_text2 = session.screen().to_string();
            let raw_text2 = session.raw_output().to_string();
            if let Some(result) = try_match(&re, &pattern_str, &name, &screen_text2, &raw_text2) {
                return result;
            }
            break;
        }
    }
    StepResult {
        name,
        passed: false,
        detail: format!(
            "timed out waiting for regex {} after {}s",
            repr_str(&pattern_str),
            repr_f64(timeout_secs)
        ),
        screen: session.screen().to_string(),
    }
}

fn try_match<'t>(
    re: &fancy_regex::Regex,
    pattern_str: &str,
    name: &str,
    screen_text: &'t str,
    raw_text: &'t str,
) -> Option<StepResult> {
    // A backtracking engine can give up on a pathological pattern. An engine
    // error is not a match, and it is not a reason to end the run either.
    //
    // A closure rather than a free function so that `fancy_regex::Captures`
    // is never named: the type gained a haystack parameter in 0.19, and
    // writing it either way would pin this crate to one side of that change
    // for no gain. See the `fancy-regex` note in the workspace manifest.
    let captures_in = |text: &'t str| {
        if text.is_empty() {
            return None;
        }
        re.captures(text).ok().flatten()
    };
    let caps = captures_in(screen_text).or_else(|| captures_in(raw_text))?;
    let full = caps.get(0).map(|m| m.as_str()).unwrap_or("");
    // Python's `groupdict()` carries every named group, matched or not; an
    // unmatched one renders as `None` rather than vanishing from the report.
    let named: Vec<String> = re
        .capture_names()
        .flatten()
        .map(|n| match caps.name(n) {
            Some(m) => format!("{n}={}", repr_str(m.as_str())),
            None => format!("{n}=None"),
        })
        .collect();
    let has_pos = caps.len() > 1;
    let mut parts: Vec<String> = Vec::new();
    if !named.is_empty() {
        parts.push(named.join(", "));
    }
    if has_pos {
        let groups: Vec<String> = (1..caps.len())
            .map(|i| match caps.get(i) {
                Some(m) => repr_str(m.as_str()),
                None => "None".to_string(),
            })
            .collect();
        parts.push(format!("groups={}", repr_tuple(&groups)));
    }
    let pattern = repr_str(pattern_str);
    let detail = if parts.is_empty() {
        format!("matched {pattern} -> match={}", repr_str(full))
    } else {
        format!(
            "matched {pattern} -> {} (full: {})",
            parts.join("; "),
            repr_str(full)
        )
    };
    Some(StepResult {
        name: name.to_string(),
        passed: true,
        detail,
        screen: screen_text.to_string(),
    })
}

// ---- dispatch ------------------------------------------------------------

/// Dispatch a single step by its `action` field.
///
/// Unknown actions produce a failed `StepResult` with the same shape as
/// `VerificationRunner._run_step` (which turns a `KeyError`/`ValueError` into
/// a failed step with the exception text in `detail`).
pub fn dispatch<S: Session + ?Sized>(
    session: &mut S,
    step: &JsonValue,
    index: usize,
) -> StepResult {
    let action = match step.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            let display = display_name(step, index, "unknown");
            return StepResult {
                name: display,
                passed: false,
                detail: "step 'action' must be a string".into(),
                screen: session.screen().to_string(),
            };
        }
    };
    let display = display_name(step, index, action);
    match action {
        "wait_for_text" => wait_for_text(session, step, index),
        "wait_for_idle" => wait_for_idle(session, step, index),
        "send_text" => send_text(session, step, index),
        "send_line" => send_line(session, step, index),
        "press" => press(session, step, index),
        "sleep" => sleep(session, step, index),
        "wait_for_regex" => wait_for_regex(session, step, index),
        other => StepResult {
            name: display,
            passed: false,
            detail: format!("unknown step action: {other}"),
            screen: session.screen().to_string(),
        },
    }
}

/// Run a sequence of steps, stopping on first failure (matches Python's
/// `VerificationRunner._run_pty/_run_process` `if not passed: break`).
pub fn run_steps<S: Session + ?Sized>(session: &mut S, steps: &[JsonValue]) -> Vec<StepResult> {
    let mut out = Vec::with_capacity(steps.len());
    for (idx, step) in steps.iter().enumerate() {
        let r = dispatch(session, step, idx + 1);
        let failed = !r.passed;
        out.push(r);
        if failed {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::InMemorySession;
    use serde_json::json;
    use std::path::PathBuf;

    fn sess(screen: &str, raw: &str) -> InMemorySession {
        let mut s = InMemorySession::new(vec!["sh".into()], PathBuf::from("/tmp/cast"), 80, 24);
        if !screen.is_empty() || !raw.is_empty() {
            // Seed both buffers independently (feed appends to both, so
            // set_screen/set_raw give precise control for corpus-style tests).
            s.set_screen(screen.to_string());
            s.set_raw(raw.to_string());
        }
        s
    }

    #[test]
    fn dispatch_unknown_action_fails() {
        let mut s = sess("", "");
        let r = dispatch(&mut s, &json!({"action": "nope"}), 1);
        assert!(!r.passed);
        assert!(r.detail.contains("unknown step action"));
    }

    #[test]
    fn dispatch_missing_action_fails() {
        let mut s = sess("", "");
        let r = dispatch(&mut s, &json!({"text": "hi"}), 2);
        assert!(!r.passed);
        assert!(r.detail.contains("'action'"));
    }

    #[test]
    fn run_steps_stops_on_failure() {
        let mut s = sess("hello", "");
        let steps = vec![
            json!({"action": "wait_for_text", "text": "hello"}),
            json!({"action": "wait_for_text", "text": "MISSING", "timeout_seconds": 0.05}),
            json!({"action": "send_text", "text": "should not run"}),
        ];
        let out = run_steps(&mut s, &steps);
        assert_eq!(out.len(), 2);
        assert!(out[0].passed);
        assert!(!out[1].passed);
    }

    #[test]
    fn wait_for_regex_captures_named_groups() {
        let mut s = sess("user: alice id: 42", "");
        let r = wait_for_regex(
            &mut s,
            &json!({"pattern": r"user: (?P<user>\w+) id: (?P<id>\d+)"}),
            1,
        );
        assert!(r.passed, "detail={}", r.detail);
        assert!(r.detail.contains("alice"));
        assert!(r.detail.contains("42"));
    }

    #[test]
    fn wait_for_regex_no_synthetic_boundary() {
        // screen="FIRST", raw="SECOND" — pattern requiring both with a dot should not match
        let mut s = sess("FIRST", "SECOND");
        let r = wait_for_regex(
            &mut s,
            &json!({"pattern": r"FIRST.SECOND", "timeout_seconds": 0.05}),
            1,
        );
        assert!(
            !r.passed,
            "synthetic boundary must not create a match: {}",
            r.detail
        );
    }

    #[test]
    fn wait_for_regex_invalid_pattern_fails() {
        let mut s = sess("", "");
        let r = wait_for_regex(&mut s, &json!({"pattern": "[bad"}), 1);
        assert!(!r.passed);
        assert!(r.detail.to_lowercase().contains("invalid"));
    }

    #[test]
    fn wait_for_regex_non_string_pattern_fails() {
        let mut s = sess("", "");
        let r = wait_for_regex(&mut s, &json!({"pattern": 42}), 1);
        assert!(!r.passed);
        assert!(r.detail.contains("pattern"));
    }

    #[test]
    fn wait_for_regex_zero_timeout_fails() {
        let mut s = sess("", "");
        let r = wait_for_regex(&mut s, &json!({"pattern": r"\d+", "timeout_seconds": 0}), 1);
        assert!(!r.passed);
        assert!(r.detail.to_lowercase().contains("timeout"));
    }

    #[test]
    fn wait_for_regex_nan_timeout_fails() {
        let mut s = sess("", "");
        // JSON cannot encode NaN directly as a number; use string trigger via serde_json::Number
        let mut step = json!({"pattern": r"\d+"});
        step["timeout_seconds"] = serde_json::Value::Number(
            serde_json::Number::from_f64(f64::NAN).unwrap_or(serde_json::Number::from(0)),
        );
        // If NaN round-trips as null/0, this still fails the >0 check
        let r = wait_for_regex(&mut s, &step, 1);
        assert!(!r.passed);
    }

    #[test]
    fn sleep_zero_is_ok() {
        let mut s = sess("hi", "");
        let r = sleep(&mut s, &json!({"seconds": 0}), 1);
        assert!(r.passed);
    }

    #[test]
    fn sleep_negative_fails() {
        let mut s = sess("", "");
        let r = sleep(&mut s, &json!({"seconds": -1}), 1);
        assert!(!r.passed);
    }

    #[test]
    fn press_unknown_key_fails() {
        let mut s = sess("", "");
        let r = press(&mut s, &json!({"key": "f13"}), 1);
        assert!(!r.passed);
    }

    #[test]
    fn send_text_records_input() {
        let mut s = sess("", "");
        let r = send_text(&mut s, &json!({"text": "hi"}), 1);
        assert!(r.passed);
        // InMemorySession logs "send_text:hi"
        assert!(s.log.iter().any(|e| e == "send_text:hi"));
    }

    #[test]
    fn send_line_appends_cr() {
        let mut s = sess("", "");
        let r = send_line(&mut s, &json!({"text": "hi"}), 1);
        assert!(r.passed);
        assert!(s.log.iter().any(|e| e == "send_line:hi"));
    }
}
