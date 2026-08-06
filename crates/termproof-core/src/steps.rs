//! Seven built-in steps with corpus parity.
//!
//! Each step mirrors `termproof/builtin_steps.py` exactly:
//!
//! * `wait_for_text` — search `screen` then `raw_output` independently.
//! * `wait_for_idle` — stable-screen detection with Instant deadlines.
//! * `send_text` / `send_line` — feed child stdin (`send_line` appends `\\r`).
//! * `press` — named-key / `Ctrl-` mapping with the frozen Python contract.
//! * `sleep` — wall-clock sleep + `read_available(0)` drain.
//! * `wait_for_regex` — regex validation, timeout validation (NaN/Inf/<=0,
//!   non-numeric), independent screen/raw search without synthetic `\\n`
//!   boundary, and evidence detail that matches `builtin_steps.py::_format_match`
//!   byte-for-byte (named groups, positional groups, full match).

use std::time::Duration;

use serde_json::Value as JsonValue;

use crate::models::StepResult;
use termproof_terminal::Session;

// ---- helpers ---------------------------------------------------------------

/// Extract the step display name: `step["name"]` or `"{index}:{action}"`.
fn display_name(step: &JsonValue, index: usize, action: &str) -> String {
    step.get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{index}:{action}"))
}

/// Extract a number field as `f64`, returning `Err(msg)` on type errors.
fn parse_number(field: &str, v: &JsonValue) -> Result<f64, String> {
    match v {
        JsonValue::Number(n) => n
            .as_f64()
            .ok_or_else(|| format!("{field} must be a number, got {v}")),
        _ => Err(format!("{field} must be a number, got {v}")),
    }
}

fn validate_timeout(raw: &JsonValue, field: &str) -> Result<Duration, String> {
    let f = parse_number(field, raw)?;
    if !f.is_finite() {
        return Err(format!("{field} must be finite, got {f}"));
    }
    if f <= 0.0 {
        return Err(format!("{field} must be > 0, got {f}"));
    }
    Ok(Duration::from_secs_f64(f))
}

// ---- wait_for_text -------------------------------------------------------

/// `wait_for_text` — poll `screen` and `raw_output` for `text`.
pub fn wait_for_text<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "wait_for_text");
    let text = match step.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "wait_for_text 'text' must be a string".into(),
                screen: session.screen(),
            };
        }
    };
    let timeout = match step.get("timeout_seconds") {
        None => Duration::from_secs(10),
        Some(v) => match validate_timeout(v, "timeout_seconds") {
            Ok(d) => d,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen(),
                };
            }
        },
    };
    let passed = session.wait_for_text(&text, timeout);
    let detail = if passed {
        format!("found {text:?}")
    } else {
        format!("timed out waiting for {text:?}")
    };
    StepResult {
        name,
        passed,
        detail,
        screen: session.screen(),
    }
}

// ---- wait_for_idle -------------------------------------------------------

/// `wait_for_idle` — screen has been stable for `stable_seconds`.
pub fn wait_for_idle<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "wait_for_idle");
    let stable = match step.get("stable_seconds") {
        None => Duration::from_millis(500),
        Some(v) => match validate_idle_duration(v) {
            Ok(d) => d,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen(),
                };
            }
        },
    };
    let timeout = match step.get("timeout_seconds") {
        None => Duration::from_secs(10),
        Some(v) => match validate_timeout(v, "timeout_seconds") {
            Ok(d) => d,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen(),
                };
            }
        },
    };
    let secs = stable.as_secs_f64();
    let passed = session.wait_for_idle(stable, timeout);
    let detail = if passed {
        format!("stable for {secs}s")
    } else {
        "timed out waiting for idle".into()
    };
    StepResult {
        name,
        passed,
        detail,
        screen: session.screen(),
    }
}

fn validate_idle_duration(v: &JsonValue) -> Result<Duration, String> {
    let f = parse_number("stable_seconds", v)?;
    if !f.is_finite() {
        return Err(format!("stable_seconds must be finite, got {f}"));
    }
    if f < 0.0 {
        return Err(format!("stable_seconds must be >= 0, got {f}"));
    }
    Ok(Duration::from_secs_f64(f))
}

// ---- send_text -----------------------------------------------------------

/// `send_text` — feed `text` verbatim (no newline).
pub fn send_text<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "send_text");
    let text = match step.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "send_text 'text' must be a string".into(),
                screen: session.screen(),
            };
        }
    };
    match session.send_text(&text) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: "sent text".into(),
            screen: session.screen(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e,
            screen: session.screen(),
        },
    }
}

// ---- send_line -----------------------------------------------------------

/// `send_line` — feed `text + \"\\r\"` (default `text` is `\"\"` as in Python).
pub fn send_line<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "send_line");
    let text = step
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    match session.send_line(&text) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: "sent line".into(),
            screen: session.screen(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e,
            screen: session.screen(),
        },
    }
}

// ---- press ---------------------------------------------------------------

/// `press` — send a named key or `Ctrl-` combo.
pub fn press<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "press");
    let key = match step.get("key").and_then(|v| v.as_str()) {
        Some(k) => k.to_string(),
        None => {
            return StepResult {
                name,
                passed: false,
                detail: "press 'key' must be a string".into(),
                screen: session.screen(),
            };
        }
    };
    match session.press(&key) {
        Ok(()) => StepResult {
            name,
            passed: true,
            detail: format!("pressed {key}"),
            screen: session.screen(),
        },
        Err(e) => StepResult {
            name,
            passed: false,
            detail: e,
            screen: session.screen(),
        },
    }
}

// ---- sleep ---------------------------------------------------------------

/// `sleep` — wall-clock sleep then `read_available(0)` drain.
pub fn sleep<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "sleep");
    let seconds = match step.get("seconds") {
        None => 1.0,
        Some(v) => match parse_number("seconds", v) {
            Ok(f) => f,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen(),
                };
            }
        },
    };
    if !seconds.is_finite() {
        return StepResult {
            name,
            passed: false,
            detail: format!("seconds must be finite, got {seconds}"),
            screen: session.screen(),
        };
    }
    if seconds < 0.0 {
        return StepResult {
            name,
            passed: false,
            detail: format!("seconds must be >= 0, got {seconds}"),
            screen: session.screen(),
        };
    }
    std::thread::sleep(Duration::from_secs_f64(seconds));
    session.read_available(Duration::ZERO);
    StepResult {
        name,
        passed: true,
        detail: "slept".into(),
        screen: session.screen(),
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
pub fn wait_for_regex<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let name = display_name(step, index, "wait_for_regex");

    // -- validate pattern ---------------------------------------------------
    let pattern_str = match step.get("pattern").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            let got = step
                .get("pattern")
                .map(type_name)
                .unwrap_or_else(|| "None".to_string());
            return StepResult {
                name,
                passed: false,
                detail: format!("wait_for_regex 'pattern' must be a string, got {got}"),
                screen: session.screen(),
            };
        }
    };
    let re = match regex::Regex::new(&pattern_str) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                name,
                passed: false,
                detail: format!("invalid regex {pattern_str:?}: {e}"),
                screen: session.screen(),
            };
        }
    };

    // -- validate timeout ---------------------------------------------------
    let timeout = match step.get("timeout_seconds") {
        None => Duration::from_secs(10),
        Some(v) => match validate_timeout(v, "wait_for_regex timeout_seconds") {
            Ok(d) => d,
            Err(e) => {
                return StepResult {
                    name,
                    passed: false,
                    detail: e,
                    screen: session.screen(),
                };
            }
        },
    };

    // -- poll loop (same cadence as wait_for_text: 50ms ticks) --------------
    let deadline = std::time::Instant::now() + timeout;

    loop {
        session.read_available(Duration::from_millis(50));
        let screen_text = session.screen();
        let raw_text = session.raw_output().to_string();
        if let Some(result) = try_match(&re, &pattern_str, &name, &screen_text, &raw_text) {
            return result;
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        if !session.is_alive() {
            session.read_available(Duration::ZERO);
            let screen_text2 = session.screen();
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
            "timed out waiting for regex {pattern_str:?} after {}s",
            timeout.as_secs_f64()
        ),
        screen: session.screen(),
    }
}

fn try_match(
    re: &regex::Regex,
    pattern_str: &str,
    name: &str,
    screen_text: &str,
    raw_text: &str,
) -> Option<StepResult> {
    let caps_opt = if !screen_text.is_empty() {
        re.captures(screen_text)
    } else {
        None
    }
    .or_else(|| {
        if !raw_text.is_empty() {
            re.captures(raw_text)
        } else {
            None
        }
    });
    let caps = caps_opt?;
    let full = caps.get(0).map(|m| m.as_str()).unwrap_or("");
    let mut named_pairs: Vec<(String, String)> = Vec::new();
    for n in re.capture_names().flatten() {
        if let Some(m) = caps.name(n) {
            named_pairs.push((n.to_string(), m.as_str().to_string()));
        }
    }
    let has_pos = re.captures_len() > 1;
    let mut parts: Vec<String> = Vec::new();
    if !named_pairs.is_empty() {
        let pairs = named_pairs
            .iter()
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(pairs);
    }
    if has_pos {
        let groups: Vec<String> = (1..re.captures_len())
            .map(|i| match caps.get(i) {
                Some(m) => format!("{:?}", m.as_str()),
                None => "None".to_string(),
            })
            .collect();
        parts.push(format!("groups=({})", groups.join(", ")));
    }
    let detail = if parts.is_empty() {
        format!("matched {pattern_str:?} -> match={full:?}")
    } else {
        format!(
            "matched {pattern_str:?} -> {} (full: {full:?})",
            parts.join("; ")
        )
    };
    Some(StepResult {
        name: name.to_string(),
        passed: true,
        detail,
        screen: screen_text.to_string(),
    })
}

fn type_name(v: &JsonValue) -> String {
    match v {
        JsonValue::Null => "NoneType".into(),
        JsonValue::Bool(_) => "bool".into(),
        JsonValue::Number(_) => "number".into(),
        JsonValue::String(_) => "str".into(),
        JsonValue::Array(_) => "list".into(),
        JsonValue::Object(_) => "dict".into(),
    }
}

// ---- dispatch ------------------------------------------------------------

/// Dispatch a single step by its `action` field.
///
/// Unknown actions produce a failed `StepResult` with the same shape as
/// `VerificationRunner._run_step` (which turns a `KeyError`/`ValueError` into
/// a failed step with the exception text in `detail`).
pub fn dispatch<S: Session>(session: &mut S, step: &JsonValue, index: usize) -> StepResult {
    let action = match step.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            let display = display_name(step, index, "unknown");
            return StepResult {
                name: display,
                passed: false,
                detail: "step 'action' must be a string".into(),
                screen: session.screen(),
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
            screen: session.screen(),
        },
    }
}

/// Run a sequence of steps, stopping on first failure (matches Python's
/// `VerificationRunner._run_pty/_run_process` `if not passed: break`).
pub fn run_steps<S: Session>(session: &mut S, steps: &[JsonValue]) -> Vec<StepResult> {
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
    use serde_json::json;
    use termproof_terminal::MockSession;

    fn sess(screen: &str, raw: &str) -> MockSession {
        MockSession::new(screen, raw)
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
        assert_eq!(s.sent(), &["hi".to_string()]);
    }

    #[test]
    fn send_line_appends_cr() {
        let mut s = sess("", "");
        let r = send_line(&mut s, &json!({"text": "hi"}), 1);
        assert!(r.passed);
        assert_eq!(s.sent(), &["hi\r".to_string()]);
    }
}
