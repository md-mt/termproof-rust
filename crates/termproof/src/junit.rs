//! JUnit XML reports — one `<testcase>` per [`RunResult`].
//!
//! Mirrors the JUnit half of `termproof/report.py` and
//! `termproof/builtin_reporters.py`. Serialization goes through `quick-junit`,
//! which guarantees valid XML and proper escaping of terminal control
//! characters.
//!
//! This is a root module rather than part of [`evidence`](crate::evidence)
//! because it is not part of the evidence pipeline: it reads a [`RunResult`]
//! and nothing else, and it renders no screen, no still and no video. Keeping
//! it here is what lets the `junit` feature stand on its own without dragging
//! `image` and `avt` in behind it (#34). [`evidence::report`](crate::evidence)
//! re-exports [`generate_junit`] so the path it has always had still resolves.

use crate::RunResult;

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
    fn junit_is_valid_xml() {
        let r = sample_result(false);
        let xml = generate_junit(&[r]);
        assert!(xml.contains("<?xml"));
        assert!(xml.contains("testsuite"));
        // Should not contain forbidden control chars.
        assert!(!xml.contains('\x01'));
    }

    #[test]
    fn junit_needs_no_artifacts() {
        // The evidence pipeline is what fills `artifacts`; JUnit reads a
        // `RunResult` and nothing else, which is why `junit` does not imply
        // `evidence` (#34).
        let mut r = sample_result(true);
        r.artifacts.clear();
        let xml = generate_junit(&[r]);
        assert!(xml.contains("<?xml"));
        assert!(xml.contains("demo"));
        assert!(!xml.contains("Artifacts:"));
    }
}
