//! Canonical result model — mirrors `termproof/models.py`.
//!
//! JSON keys use `snake_case` to preserve byte-stable serialization.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Result of a single step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    /// Step display name.
    pub name: String,
    /// Whether the step passed.
    pub passed: bool,
    /// Human-readable detail.
    pub detail: String,
    /// Screen snapshot at step boundary (normalized text).
    pub screen: String,
}

/// Result of a single assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionResult {
    /// Assertion name / type.
    pub name: String,
    /// Whether the assertion passed.
    pub passed: bool,
    /// Human-readable detail.
    pub detail: String,
}

/// Canonical verification result — one recipe × one renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// Recipe identifier.
    pub recipe_name: String,
    /// Overall pass/fail.
    pub passed: bool,
    /// Process exit code, if available.
    pub exit_code: Option<i32>,
    /// Wall-clock duration in seconds.
    pub duration_seconds: f64,
    /// Priority label (P0/P1/P2).
    pub priority: String,
    /// Execution mode (`scripted`, `agent-driven`, …).
    pub execution: String,
    /// Renderer name (`default`, …).
    pub renderer: String,
    /// Score `0.0..1.0`.
    pub score: f64,
    /// Per-step results in order.
    pub steps: Vec<StepResult>,
    /// Per-assertion results.
    pub assertions: Vec<AssertionResult>,
    /// Artifact map: logical name → absolute path string.
    pub artifacts: BTreeMap<String, String>,
}

impl RunResult {
    /// Compute score from assertion results (all pass → 1.0, else fraction).
    pub fn score_from_assertions(assertions: &[AssertionResult]) -> f64 {
        if assertions.is_empty() {
            return 1.0;
        }
        let passed = assertions.iter().filter(|a| a.passed).count();
        if passed == assertions.len() {
            1.0
        } else {
            passed as f64 / assertions.len() as f64
        }
    }

    /// Serialize to canonical JSON value (BTreeMap ensures stable key ordering).
    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("RunResult is serializable")
    }

    /// Serialize to pretty-printed JSON string with trailing newline.
    pub fn to_json_pretty(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).expect("RunResult is serializable");
        s.push('\n');
        s
    }
}
