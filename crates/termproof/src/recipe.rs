//! Typed recipe models with flattened extension maps and legacy loading.
//!
//! The models mirror `termproof/models.py` but preserve unknown fields via
//! flattened `extension` maps rather than discarding them. Additional
//! properties are permitted at the recipe, command, step, and assertion level
//! (see JSON Schema `additionalProperties: true`).

use std::collections::HashMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The current canonical recipe version.
pub const RECIPE_VERSION: u32 = 1;

fn default_recipe_version() -> u32 {
    RECIPE_VERSION
}

fn default_description() -> String {
    String::new()
}

fn default_intent() -> String {
    String::new()
}

fn default_priority() -> String {
    "P2".to_string()
}

fn default_execution() -> String {
    "scripted".to_string()
}

fn default_determinism() -> String {
    "deterministic".to_string()
}

fn default_timeout_seconds() -> f64 {
    30.0
}

fn default_cols() -> u32 {
    100
}

fn default_rows() -> u32 {
    30
}

fn default_expect_exit_code() -> Option<i32> {
    Some(0)
}

fn default_pty() -> bool {
    true
}

/// Human-friendly default for `renderers`: `{"default": []}`.
fn default_renderers() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert("default".to_string(), Vec::new());
    m
}

/// Command specification for a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandSpec {
    /// Target command and arguments; at least one entry required.
    pub argv: Vec<String>,

    /// Working directory for the command; `None` means inherited.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub cwd: Option<String>,

    /// Environment variables for the command.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether to allocate a PTY for the command.
    #[serde(default = "default_pty")]
    pub pty: bool,

    /// Extension fields not covered by the typed schema (`additionalProperties: true`).
    #[serde(default, flatten)]
    #[schemars(flatten)]
    pub extension: HashMap<String, serde_json::Value>,
}

/// A single step in a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Step {
    /// Action name, e.g. `wait_for_text`, `send_line`.
    pub action: String,

    /// Optional human-readable step name.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub name: Option<String>,

    /// Per-step timeout in seconds, if overridden.
    #[serde(default)]
    #[schemars(with = "Option<f64>")]
    pub timeout_seconds: Option<f64>,

    /// Any additional step fields (e.g. `text`, `key`, `pattern`).
    #[serde(default, flatten)]
    #[schemars(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A single assertion in a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Assertion {
    /// Assertion type, e.g. `output_contains`.
    #[serde(rename = "type")]
    pub kind: String,

    /// Optional human-readable assertion name.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    pub name: Option<String>,

    /// Any additional assertion fields (e.g. `value`, `path`, `schema`).
    #[serde(default, flatten)]
    #[schemars(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Typed recipe model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Recipe {
    /// Recipe format version; defaults to `1` for legacy recipes.
    #[serde(default = "default_recipe_version")]
    #[schemars(range(min = 1, max = 1))]
    pub recipe_version: u32,

    /// Human-readable recipe identifier.
    pub name: String,

    /// Human-readable description.
    #[serde(default = "default_description")]
    pub description: String,

    /// Intent description.
    #[serde(default = "default_intent")]
    pub intent: String,

    /// Priority label, e.g. `P2`.
    #[serde(default = "default_priority")]
    pub priority: String,

    /// Execution mode name, e.g. `scripted` or `agent-driven`.
    #[serde(default = "default_execution")]
    pub execution: String,

    /// Determinism label.
    #[serde(default = "default_determinism")]
    pub determinism: String,

    /// CI path filters.
    #[serde(default)]
    pub ci_paths: Vec<String>,

    /// Checks list (human-readable expectations).
    #[serde(default)]
    pub checks: Vec<String>,

    /// Operator configuration (free-form).
    #[serde(default)]
    pub operator: HashMap<String, serde_json::Value>,

    /// Renderer table, e.g. `{"default": []}`.
    #[serde(default = "default_renderers")]
    pub renderers: HashMap<String, Vec<String>>,

    /// Command to execute.
    pub command: CommandSpec,

    /// Ordered steps to drive the session.
    #[serde(default)]
    pub steps: Vec<Step>,

    /// Assertions to evaluate after execution.
    #[serde(default)]
    pub assertions: Vec<Assertion>,

    /// Expected exit code; `None` means no expectation.
    #[serde(default = "default_expect_exit_code")]
    #[schemars(with = "Option<i32>")]
    pub expect_exit_code: Option<i32>,

    /// Overall recipe timeout in seconds.
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: f64,

    /// Terminal columns.
    #[serde(default = "default_cols")]
    pub cols: u32,

    /// Terminal rows.
    #[serde(default = "default_rows")]
    pub rows: u32,

    /// Source path for diagnostics; not serialized.
    #[serde(skip, default)]
    #[schemars(skip)]
    pub source_path: Option<String>,

    /// Extension fields not covered by the typed schema.
    #[serde(default, flatten)]
    #[schemars(flatten)]
    pub extension: HashMap<String, serde_json::Value>,
}

impl Recipe {
    /// Load a recipe from a file path, supporting both JSON and YAML inputs.
    ///
    /// JSON is tried first (preserving number fidelity for the legacy
    /// integral-float check); on failure the content is parsed as YAML. This
    /// matches the spec requirement that both formats are accepted and keeps
    /// backward compatibility with the Python loader which only read JSON.
    pub fn from_file(path: &Path) -> Result<Self, crate::error::CoreError> {
        let content = std::fs::read_to_string(path).map_err(|e| crate::error::CoreError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        Self::from_str(&content, Some(path))
    }

    /// Parse a recipe from a string, optionally attaching a source path.
    ///
    /// JSON and YAML are both accepted (JSON first, YAML fallback).
    pub fn from_str(content: &str, source: Option<&Path>) -> Result<Self, crate::error::CoreError> {
        // Attempt JSON first for fidelity; fall back to YAML.
        let mut recipe: Self = serde_json::from_str(content)
            .or_else(|_| serde_yaml::from_str(content))
            .map_err(|e| crate::error::CoreError::Parse {
                path: source
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<string>".to_string()),
                message: e.to_string(),
            })?;
        if let Some(p) = source {
            recipe.source_path = Some(p.display().to_string());
        }
        // Normalize: if the raw input omitted recipe_version, the deserialized
        // value will be the default 1. The caller can detect legacy via
        // `was_recipe_version_missing` below if needed for warnings.
        Ok(recipe)
    }

    /// Whether the raw value for `recipe_version` was missing (legacy recipe).
    ///
    /// This helper re-parses the raw JSON/YAML to check presence, because
    /// deserialization defaults to 1. It is used by validation to emit the
    /// legacy warning without failing the recipe.
    pub fn was_recipe_version_missing(raw: &serde_json::Value) -> bool {
        !raw.as_object()
            .map(|o| o.contains_key("recipe_version"))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_recipe_defaults_version_to_one() {
        let recipe: Recipe =
            serde_json::from_str(r#"{"name":"x","command":{"argv":["true"]}}"#).expect("parse");
        assert_eq!(recipe.recipe_version, 1);
        assert_eq!(recipe.priority, "P2");
        assert_eq!(recipe.cols, 100);
        assert_eq!(recipe.timeout_seconds, 30.0);
        assert_eq!(recipe.expect_exit_code, Some(0));
    }

    #[test]
    fn extension_fields_are_preserved() {
        let recipe: Recipe = serde_json::from_str(
            r#"{"name":"x","command":{"argv":["true"],"custom":"keep"},"my_extra":"hello","recipe_version":1}"#,
        )
        .expect("parse");
        assert_eq!(recipe.extension.get("my_extra").unwrap(), "hello");
        assert_eq!(recipe.command.extension.get("custom").unwrap(), "keep");
    }

    #[test]
    fn json_and_yaml_both_load() {
        let json_str = r#"{"name":"x","command":{"argv":["echo","hi"]},"recipe_version":1}"#;
        let yaml_str = "name: x\ncommand:\n  argv: [echo, hi]\nrecipe_version: 1\n";
        let from_json = Recipe::from_str(json_str, None).expect("json");
        let from_yaml = Recipe::from_str(yaml_str, None).expect("yaml");
        assert_eq!(from_json.name, "x");
        assert_eq!(from_yaml.name, "x");
        assert_eq!(from_json.command.argv, from_yaml.command.argv);
    }

    #[test]
    fn step_extra_preserved() {
        let recipe: Recipe = serde_json::from_str(
            r#"{"name":"x","command":{"argv":["true"]},"steps":[{"action":"wait_for_text","text":"hello","timeout_seconds":5}],"recipe_version":1}"#,
        )
        .expect("parse");
        assert_eq!(recipe.steps[0].action, "wait_for_text");
        assert_eq!(recipe.steps[0].extra.get("text").unwrap(), "hello");
    }
}
