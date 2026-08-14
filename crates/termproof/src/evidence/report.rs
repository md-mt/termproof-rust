//! Unified report pipeline — Markdown and JUnit from one `RunResult` model.
//!
//! Mirrors `termproof/report.py` and `termproof/builtin_reporters.py` but
//! collapses the duplicated helper logic identified in #80 into a single
//! module.  JUnit is serialized via `quick-junit`, guaranteeing valid XML
//! and proper escaping of terminal control characters.

use crate::RunResult;

// ---------------------------------------------------------------------------
// Shared helpers (single source for both reporters)
// ---------------------------------------------------------------------------

fn evidence_links(result: &RunResult) -> String {
    let order = [
        "screenshot",
        "visual_diff",
        "visual_baseline",
        "video",
        "cast",
        "screen_text",
        "step_screenshots",
    ];
    let links: Vec<String> = order
        .iter()
        .filter_map(|k| result.artifacts.get(*k).map(|v| format!("[{k}]({v})")))
        .collect();
    if links.is_empty() {
        "-".to_string()
    } else {
        links.join(" / ")
    }
}

#[allow(dead_code)]
fn one_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/// Generate a Markdown aggregate report from `results`.
///
/// When `results` is a single item, the per-recipe detail sections are still
/// included so CI summaries are self-contained.
pub fn generate_markdown(results: &[RunResult]) -> String {
    let passed = results.iter().filter(|r| r.passed).count();
    let mut lines = vec![
        format!("# TUI Verification - {passed}/{} Passed", results.len()),
        String::new(),
    ];

    lines.push(
        "| Recipe | Renderer | Priority | Execution | Result | Score | Evidence |".to_string(),
    );
    lines.push("| --- | --- | --- | --- | --- | --- | --- |".to_string());

    for result in results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        let evidence = evidence_links(result);
        lines.push(format!(
            "| `{}` | `{}` | `{}` | `{}` | {status} | {:.2} | {evidence} |",
            result.recipe_name, result.renderer, result.priority, result.execution, result.score
        ));
    }
    for result in results {
        lines.push(String::new());
        lines.push(detail_markdown(result));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn detail_markdown(result: &RunResult) -> String {
    let status = if result.passed { "PASS" } else { "FAIL" };
    let mut lines = vec![
        format!(
            "<details><summary>{status} {} [{}]</summary>",
            result.recipe_name, result.renderer
        ),
        String::new(),
        "### Assertions".to_string(),
        String::new(),
    ];
    for a in &result.assertions {
        let mark = if a.passed { "PASS" } else { "FAIL" };
        lines.push(format!("- {mark} `{}` - {}", a.name, a.detail));
    }
    lines.push(String::new());
    lines.push("### Steps".to_string());
    lines.push(String::new());
    for s in &result.steps {
        let mark = if s.passed { "PASS" } else { "FAIL" };
        lines.push(format!("- {mark} `{}` - {}", s.name, s.detail));
    }
    lines.push(String::new());
    lines.push("</details>".to_string());
    lines.join("\n")
}

/// Per-run Markdown report (single result).
pub fn generate_markdown_single(result: &RunResult) -> String {
    generate_markdown(std::slice::from_ref(result))
}

/// CLI summary line: `X/Y passed` plus report path.
pub fn cli_summary(results: &[RunResult]) -> String {
    let passed = results.iter().filter(|r| r.passed).count();
    format!("{passed}/{} passed", results.len())
}

// ---------------------------------------------------------------------------
// JUnit via quick-junit
// ---------------------------------------------------------------------------

/// Generate a JUnit XML report from `results`.
///
/// Each `RunResult` maps to one `<testcase>`.  Failures include step +
/// assertion diagnostics.  `system-out` always contains the step/assertion
/// summary and artifact list for CI visibility, even for passing cases.
/// Invalid XML characters are sanitized by `quick-junit` and the local
/// `xml_sanitize` helper for pre-serialization safety.
pub fn generate_junit(results: &[RunResult]) -> String {
    use quick_junit::{Report, TestCase, TestCaseStatus, TestSuite};

    let mut report = Report::new("termproof");
    let mut suite = TestSuite::new("termproof");

    for result in results {
        let tc_name = if result.renderer != "default" {
            format!("{} [{}]", result.recipe_name, result.renderer)
        } else {
            result.recipe_name.clone()
        };
        // tc created per-branch below

        // Build diagnostics string.
        let mut failure_lines: Vec<String> = Vec::new();
        let mut stdout_lines: Vec<String> = Vec::new();

        // Steps
        if !result.steps.is_empty() {
            if !result.passed {
                failure_lines.push("Steps:".to_string());
            }
            stdout_lines.push("Steps:".to_string());
            for s in &result.steps {
                let mark = if s.passed { "PASS" } else { "FAIL" };
                let line = format!(
                    "  {mark} {}: {}",
                    xml_sanitize(&s.name),
                    xml_sanitize(&s.detail)
                );
                if !result.passed {
                    failure_lines.push(line.clone());
                }
                stdout_lines.push(line);
            }
        }
        // Assertions
        if !result.assertions.is_empty() {
            if !result.passed && !failure_lines.is_empty() {
                failure_lines.push(String::new());
            }
            if !stdout_lines.is_empty() {
                stdout_lines.push(String::new());
            }
            failure_lines.push("Assertions:".to_string());
            stdout_lines.push("Assertions:".to_string());
            for a in &result.assertions {
                let mark = if a.passed { "PASS" } else { "FAIL" };
                let line = format!(
                    "  {mark} {}: {}",
                    xml_sanitize(&a.name),
                    xml_sanitize(&a.detail)
                );
                failure_lines.push(line.clone());
                stdout_lines.push(line);
            }
        }
        // Artifacts
        if !result.artifacts.is_empty() {
            if !result.passed {
                failure_lines.push(String::new());
                failure_lines.push("Artifacts:".to_string());
            }
            stdout_lines.push(String::new());
            stdout_lines.push("Artifacts:".to_string());
            for (k, v) in &result.artifacts {
                let line = format!("  {}: {}", xml_sanitize(k), xml_sanitize(v));
                if !result.passed {
                    failure_lines.push(line.clone());
                }
                stdout_lines.push(line);
            }
        }
        let tc = if result.passed {
            let mut c = TestCase::new(tc_name.clone(), TestCaseStatus::success());
            c.set_classname(xml_sanitize(&result.execution));
            c.set_time(std::time::Duration::from_secs_f64(
                result.duration_seconds.max(0.0),
            ));
            c.set_system_out(xml_sanitize(&stdout_lines.join("\n")));
            c
        } else {
            let mut header = vec![
                format!("Recipe: {}", xml_sanitize(&result.recipe_name)),
                format!("Renderer: {}", xml_sanitize(&result.renderer)),
                format!("Priority: {}", xml_sanitize(&result.priority)),
                format!("Execution: {}", xml_sanitize(&result.execution)),
                format!("Score: {:.2}", result.score),
                format!("Exit code: {:?}", result.exit_code),
                String::new(),
            ];
            header.extend(failure_lines);
            let msg = format!("{} failed (score {:.2})", result.recipe_name, result.score);
            let mut status = TestCaseStatus::non_success(quick_junit::NonSuccessKind::Failure);
            status.set_message(xml_sanitize(&msg));
            status.set_type("AssertionError");
            status.set_description(xml_sanitize(&header.join("\n")));
            let mut c = TestCase::new(tc_name.clone(), status);
            c.set_classname(xml_sanitize(&result.execution));
            c.set_time(std::time::Duration::from_secs_f64(
                result.duration_seconds.max(0.0),
            ));
            c.set_system_out(xml_sanitize(&stdout_lines.join("\n")));
            c
        };
        suite.add_test_case(tc);
    }

    report.add_test_suite(suite);
    let xml = report.to_string().expect("junit serialization");
    // Ensure XML declaration present.
    if xml.starts_with("<?xml") {
        xml
    } else {
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{xml}")
    }
}

/// Strip XML 1.0 forbidden characters (control chars, surrogates, noncharacters).
fn xml_sanitize(text: &str) -> String {
    text.chars()
        .filter(|&ch| {
            matches!(ch,
                '\x09' | '\x0A' | '\x0D' |
                '\u{0020}'..='\u{D7FF}' |
                '\u{E000}'..='\u{FFFD}' |
                '\u{10000}'..='\u{10FFFF}'
            ) && !matches!(ch, '\u{FDD0}'..='\u{FDEF}' | '\u{FFFE}' | '\u{FFFF}')
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate `duration_seconds` is finite and non-negative, per RUST-001 corpus.
pub fn validate_duration(value: f64) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("duration_seconds must be finite, got {value}"));
    }
    if value < 0.0 {
        return Err(format!("duration_seconds must be >= 0, got {value}"));
    }
    Ok(())
}

/// Validate a recipe JSON blob has required `name` and `command.argv`.
pub fn validate_recipe_json(value: &serde_json::Value) -> Vec<String> {
    let mut errors = Vec::new();
    if value.get("name").and_then(|v| v.as_str()).is_none() {
        errors.push("missing required field: name".to_string());
    }
    match value.get("command") {
        Some(cmd) => {
            if cmd.get("argv").and_then(|v| v.as_array()).is_none() {
                errors.push("missing required field: command.argv".to_string());
            }
        }
        None => errors.push("missing required field: command".to_string()),
    }
    // timeout / cols / rows sanity
    if let Some(v) = value.get("timeout_seconds").and_then(|v| v.as_f64()) {
        if !v.is_finite() || v <= 0.0 {
            errors.push(format!("timeout_seconds must be > 0, got {v}"));
        }
    }
    if let Some(v) = value.get("cols").and_then(|v| v.as_u64()) {
        if v == 0 {
            errors.push("cols must be > 0".to_string());
        }
    }
    if let Some(v) = value.get("rows").and_then(|v| v.as_u64()) {
        if v == 0 {
            errors.push("rows must be > 0".to_string());
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssertionResult, RunResult, StepResult};
    use std::collections::BTreeMap;

    fn sample_result(passed: bool) -> RunResult {
        RunResult {
            result_version: Some(crate::result::RESULT_SCHEMA_VERSION),
            recipe_name: "demo".into(),
            passed,
            exit_code: Some(0),
            duration_seconds: 1.2,
            priority: "P2".into(),
            execution: "scripted".into(),
            renderer: "default".into(),
            score: if passed { 1.0 } else { 0.5 },
            steps: vec![StepResult {
                name: "step1".into(),
                passed,
                detail: "ok".into(),
                screen: "hello".into(),
            }],
            assertions: vec![AssertionResult {
                name: "exit_code".into(),
                passed,
                detail: "exit 0".into(),
            }],
            artifacts: BTreeMap::from([("screenshot".to_string(), "/tmp/final.svg".to_string())]),
        }
    }

    #[test]
    fn markdown_contains_header() {
        let r = sample_result(true);
        let md = generate_markdown(&[r]);
        assert!(md.contains("# TUI Verification"));
        assert!(md.contains("demo"));
    }

    #[test]
    fn junit_is_valid_xml() {
        let r = sample_result(false);
        let xml = generate_junit(&[r]);
        assert!(xml.contains("<?xml"));
        assert!(xml.contains("testsuite"));
        // Should not contain forbidden control chars.
        assert!(!xml.contains('\x01'));
    }

    #[test]
    fn duration_validation() {
        assert!(validate_duration(1.0).is_ok());
        assert!(validate_duration(f64::NAN).is_err());
        assert!(validate_duration(f64::INFINITY).is_err());
        assert!(validate_duration(-0.1).is_err());
    }
}
