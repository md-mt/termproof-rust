//! Report pipeline — Markdown and recipe validation from one `RunResult` model.
//!
//! Mirrors `termproof/report.py` and `termproof/builtin_reporters.py` but
//! collapses the duplicated helper logic identified in #80 into a single
//! module.
//!
//! The JUnit writer is in [`crate::junit`], behind the `junit` feature: it
//! reads a `RunResult` and nothing else, and shares no helper with the
//! Markdown one. The re-export below keeps `evidence::report::generate_junit`
//! resolving where it always did; it gates no code of its own.

use crate::RunResult;

#[cfg(feature = "junit")]
pub use crate::junit::generate_junit;

// ---------------------------------------------------------------------------
// Shared helpers
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
    fn duration_validation() {
        assert!(validate_duration(1.0).is_ok());
        assert!(validate_duration(f64::NAN).is_err());
        assert!(validate_duration(f64::INFINITY).is_err());
        assert!(validate_duration(-0.1).is_err());
    }
}
