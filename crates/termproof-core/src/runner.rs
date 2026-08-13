//! The concrete `ExecutionContext` — what actually runs a recipe.
//!
//! [`ExecutionMode`] implementations are written against `&mut dyn
//! ExecutionContext` and cannot construct a session themselves; until this
//! existed the only contexts in the workspace were contract doubles, so
//! nothing could take a recipe and produce a [`RunResult`].
//!
//! What is wired here is deliberately narrow, and the narrowness is reported
//! rather than papered over. A recipe whose `command.pty` is false, or whose
//! `execution` is not `scripted`, is refused with a diagnostic naming the
//! reason — running it on a pseudo-terminal anyway would report a verdict
//! about something the recipe did not ask for.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use termproof_terminal::{PtySessionBackend, Session, SessionBackend};

use crate::error::CoreError;
use crate::execution::{ExecutionContext, ExecutionError, ExecutionMode, ScriptedPtyMode};
use crate::models::{AssertionResult as ModelAssertion, Recipe, StepResult as ModelStep};
use crate::result::{AssertionResult, RunResult, StepResult};

/// A recipe loaded from disk, in the two shapes the run path needs.
#[derive(Debug, Clone)]
pub struct LoadedRecipe {
    /// The recipe as the execution path consumes it.
    ///
    /// Steps and assertions stay as raw JSON: `specs/001-recipe-format/spec.md`
    /// FR-013 says neither is inspected at load, and the step layer's own
    /// coercion rules are what decide whether a step is well-formed.
    pub recipe: Recipe,
    /// Renderer name → extra argv, sorted by name so plans are reproducible.
    pub renderers: Vec<(String, Vec<String>)>,
}

impl LoadedRecipe {
    /// Load a recipe file. JSON is tried first, then YAML.
    pub fn from_file(path: &Path) -> Result<Self, CoreError> {
        let content = std::fs::read_to_string(path).map_err(|e| CoreError::Io {
            path: path.display().to_string(),
            source: e,
        })?;
        let document: Value = serde_json::from_str(&content)
            .or_else(|_| serde_yaml::from_str(&content))
            .map_err(|e| CoreError::Parse {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
        let mut recipe: Recipe =
            serde_json::from_value(document.clone()).map_err(|e| CoreError::Parse {
                path: path.display().to_string(),
                message: e.to_string(),
            })?;
        recipe.source_path = Some(path.to_path_buf());
        Ok(Self {
            renderers: renderers_from(&document),
            recipe,
        })
    }
}

/// Read the renderer table, falling back to the single `default` renderer.
fn renderers_from(document: &Value) -> Vec<(String, Vec<String>)> {
    let table = match document.get("renderers").and_then(Value::as_object) {
        Some(table) if !table.is_empty() => table,
        _ => return vec![("default".to_string(), Vec::new())],
    };
    let mut renderers: Vec<(String, Vec<String>)> = table
        .iter()
        .map(|(name, argv)| {
            let argv = argv
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            (name.clone(), argv)
        })
        .collect();
    renderers.sort_by(|a, b| a.0.cmp(&b.0));
    renderers
}

/// Runs recipes against a session backend.
pub struct Runner {
    backend: Box<dyn SessionBackend>,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner {
    /// A runner that puts the target on a real pseudo-terminal.
    pub fn new() -> Self {
        Self {
            backend: Box::new(PtySessionBackend::new()),
        }
    }

    /// A runner over a supplied backend.
    pub fn with_backend(backend: Box<dyn SessionBackend>) -> Self {
        Self { backend }
    }

    /// Execute one recipe under one renderer, writing evidence into `run_dir`.
    ///
    /// The error case is a refusal to start, not a failed verification: a
    /// recipe that runs and fails its steps comes back as a `RunResult` with
    /// `passed` false. Turning a mid-run failure into a structured result is
    /// RUST-009's subject and is not attempted here.
    pub fn run(
        &mut self,
        recipe: &Recipe,
        renderer: &str,
        run_dir: &Path,
    ) -> Result<RunResult, ExecutionError> {
        let mode = select_mode(recipe)?;
        std::fs::create_dir_all(run_dir).map_err(|e| {
            ExecutionError::Config(format!("cannot create {}: {e}", run_dir.display()))
        })?;

        let started = Instant::now();
        let (steps, assertions, raw_output, exit_code, screen) =
            mode.execute(self, recipe, run_dir)?;
        let duration_seconds = started.elapsed().as_secs_f64();

        let steps: Vec<StepResult> = steps.into_iter().map(step_result).collect();
        let assertions: Vec<AssertionResult> =
            assertions.into_iter().map(assertion_result).collect();

        let mut artifacts = std::collections::BTreeMap::new();
        for (name, file, body) in [
            ("raw_output", "raw_output.txt", raw_output),
            ("screen", "screen.txt", screen),
        ] {
            let path = run_dir.join(file);
            if crate::store::atomic_write_text(&path, &body).is_ok() {
                artifacts.insert(name.to_string(), path.display().to_string());
            }
        }
        let cast = run_dir.join("session.cast");
        if cast.exists() {
            artifacts.insert("cast".to_string(), cast.display().to_string());
        }

        // 003-FR-023: a run passes only when every step and every assertion did.
        let passed = steps.iter().all(|s| s.passed) && assertions.iter().all(|a| a.passed);
        Ok(RunResult {
            recipe_name: recipe.name.clone(),
            passed,
            exit_code,
            duration_seconds,
            priority: recipe.priority.clone(),
            execution: recipe.execution.clone(),
            renderer: renderer.to_string(),
            score: RunResult::score_from_assertions(&assertions),
            steps,
            assertions,
            artifacts,
        })
    }
}

/// Pick the execution mode, or say why the recipe cannot be run.
fn select_mode(recipe: &Recipe) -> Result<Box<dyn ExecutionMode>, ExecutionError> {
    if recipe.execution != "scripted" {
        return Err(ExecutionError::Config(format!(
            "execution mode '{}' is not wired to the CLI yet; only 'scripted' runs",
            recipe.execution
        )));
    }
    if !recipe.command.pty {
        return Err(ExecutionError::Config(
            "command.pty is false, and non-pty sessions are not wired yet; \
             running the target on a pty regardless would verify something the recipe did not ask for"
                .to_string(),
        ));
    }
    Ok(Box::new(ScriptedPtyMode))
}

fn step_result(step: ModelStep) -> StepResult {
    StepResult {
        name: step.name,
        passed: step.passed,
        detail: step.detail,
        screen: step.screen,
    }
}

fn assertion_result(assertion: ModelAssertion) -> AssertionResult {
    AssertionResult {
        name: assertion.name,
        passed: assertion.passed,
        detail: assertion.detail,
    }
}

impl ExecutionContext for Runner {
    fn create_session(
        &mut self,
        argv: Vec<String>,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        cast_path: PathBuf,
    ) -> Result<Box<dyn Session>, ExecutionError> {
        self.backend
            .create_session(argv, cast_path, cwd, env, cols, rows)
            .map_err(|e| ExecutionError::Session(e.to_string()))
    }

    fn run_step(&mut self, session: &mut dyn Session, step: &Value, index: usize) -> ModelStep {
        let result = crate::steps::dispatch(session, step, index);
        ModelStep {
            name: result.name,
            passed: result.passed,
            detail: result.detail,
            screen: result.screen,
        }
    }

    /// Placeholder until the built-in assertions land (RUST-008, issue #1).
    ///
    /// `evaluate_assertion` is a required trait method, so a concrete context
    /// has to supply something; reporting `passed: false` keeps a run from
    /// claiming a verdict nobody computed. When RUST-008 gives the trait a
    /// default body, **delete this method** and the default takes over — that
    /// is the whole of the handover.
    fn evaluate_assertion(
        &self,
        _recipe: &Recipe,
        assertion: &Value,
        _screen: &str,
        _raw_output: &str,
        _exit_code: Option<i32>,
    ) -> ModelAssertion {
        let kind = assertion
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("<missing type>");
        let name = assertion
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(kind)
            .to_string();
        ModelAssertion {
            name,
            passed: false,
            detail: format!("assertion {kind} is not implemented yet (RUST-008)"),
        }
    }
}
