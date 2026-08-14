//! Canonical data models — Recipe, results, and related types.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Command to execute for a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    /// Command and arguments.
    pub argv: Vec<String>,
    /// Working directory override.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Environment overrides.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Whether to allocate a PTY.
    #[serde(default = "default_true")]
    pub pty: bool,
}

fn default_true() -> bool {
    true
}

impl Default for CommandSpec {
    fn default() -> Self {
        Self {
            argv: vec![],
            cwd: None,
            env: HashMap::new(),
            pty: true,
        }
    }
}

/// A verification recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    /// Human-readable name.
    pub name: String,
    /// Command to run.
    pub command: CommandSpec,
    /// Recipe version (must be 1).
    #[serde(default = "default_version")]
    pub recipe_version: u32,
    /// Optional description.
    #[serde(default)]
    pub description: String,
    /// Intent (higher-level description).
    #[serde(default)]
    pub intent: String,
    /// Priority.
    #[serde(default = "default_priority")]
    pub priority: String,
    /// Execution mode: "scripted" or "agent-driven".
    #[serde(default = "default_execution")]
    pub execution: String,
    /// Checks for agent-driven runs.
    #[serde(default)]
    pub checks: Vec<String>,
    /// Operator config for agent-driven runs.
    #[serde(default)]
    pub operator: HashMap<String, serde_json::Value>,
    /// Ordered steps.
    #[serde(default)]
    pub steps: Vec<serde_json::Value>,
    /// Assertions.
    #[serde(default)]
    pub assertions: Vec<serde_json::Value>,
    /// Expected exit code.
    #[serde(default = "default_expect")]
    pub expect_exit_code: Option<i32>,
    /// Timeout seconds.
    #[serde(default = "default_timeout")]
    pub timeout_seconds: f64,
    /// Terminal columns.
    #[serde(default = "default_cols")]
    pub cols: u16,
    /// Terminal rows.
    #[serde(default = "default_rows")]
    pub rows: u16,
    /// Source path (not serialized).
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

fn default_version() -> u32 {
    1
}
fn default_priority() -> String {
    "P2".to_string()
}
fn default_execution() -> String {
    "scripted".to_string()
}
fn default_expect() -> Option<i32> {
    Some(0)
}
fn default_timeout() -> f64 {
    30.0
}
fn default_cols() -> u16 {
    100
}
fn default_rows() -> u16 {
    30
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            name: String::new(),
            command: CommandSpec::default(),
            recipe_version: 1,
            description: String::new(),
            intent: String::new(),
            priority: "P2".to_string(),
            execution: "scripted".to_string(),
            checks: vec![],
            operator: HashMap::new(),
            steps: vec![],
            assertions: vec![],
            expect_exit_code: Some(0),
            timeout_seconds: 30.0,
            cols: 100,
            rows: 30,
            source_path: None,
        }
    }
}

/// Result of a single step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    /// Step name.
    pub name: String,
    /// Whether it passed.
    pub passed: bool,
    /// Detail / diagnostic.
    pub detail: String,
    /// Screen snapshot.
    pub screen: String,
}

/// Result of a single assertion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssertionResult {
    /// Assertion name.
    pub name: String,
    /// Whether it passed.
    pub passed: bool,
    /// Detail / diagnostic.
    pub detail: String,
}

/// Aggregated run result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// Recipe name.
    pub recipe_name: String,
    /// Whether the whole run passed.
    pub passed: bool,
    /// Exit code.
    pub exit_code: Option<i32>,
    /// Duration seconds.
    pub duration_seconds: f64,
    /// Priority.
    pub priority: String,
    /// Execution mode.
    pub execution: String,
    /// Renderer.
    pub renderer: String,
    /// Score 0..1.
    pub score: f64,
    /// Step results.
    pub steps: Vec<StepResult>,
    /// Assertion results.
    pub assertions: Vec<AssertionResult>,
    /// Artifact paths.
    pub artifacts: HashMap<String, String>,
}
