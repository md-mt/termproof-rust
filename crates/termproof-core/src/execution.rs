//! ExecutionContext and ExecutionMode public traits (RUST-016).
//!
//! Execution modes receive an `ExecutionContext` that exposes only supported
//! operations (session creation, step dispatch, assertion evaluation). They
//! must not call private runner methods. This closes #78.
//!
//! Built-in modes are migrated to use the context.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::models::{AssertionResult, Recipe, StepResult};

/// Result type for execution modes.
pub type ExecutionResult = (
    Vec<StepResult>,
    Vec<AssertionResult>,
    String,
    Option<i32>,
    String,
);

/// Errors from execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    /// Step failed to execute.
    #[error("step error: {0}")]
    Step(String),
    /// Session backend error.
    #[error("session error: {0}")]
    Session(String),
    /// Configuration or recipe error.
    #[error("config error: {0}")]
    Config(String),
    /// Timeout.
    #[error("timeout: {0}")]
    Timeout(String),
}

/// The recipe's declared assertions plus the implicit `expect_exit_code` one.
///
/// Shared by the two `evaluate_assertions*` default bodies so the assertion
/// list cannot drift between the step-aware and step-blind paths.
fn assertions_with_implicit(recipe: &Recipe) -> Vec<Value> {
    let mut all = recipe.assertions.clone();
    if let Some(expected) = recipe.expect_exit_code {
        all.push(serde_json::json!({"type": "exit_code", "value": expected}));
    }
    all
}

/// Public context passed to execution modes.
///
/// This is the only interface execution modes may use. It hides runner
/// internals and makes the boundary testable with an `InMemory` fake.
pub trait ExecutionContext: Send {
    /// Create a new terminal session via the configured backend.
    fn create_session(
        &mut self,
        argv: Vec<String>,
        cwd: Option<String>,
        env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        cast_path: PathBuf,
    ) -> Result<Box<dyn termproof_terminal::Session>, ExecutionError>;

    /// Execute a single step through the registered step registry.
    fn run_step(
        &mut self,
        session: &mut dyn termproof_terminal::Session,
        step: &Value,
        index: usize,
    ) -> StepResult;

    /// Evaluate a single assertion.
    fn evaluate_assertion(
        &self,
        recipe: &Recipe,
        assertion: &Value,
        screen: &str,
        raw_output: &str,
        exit_code: Option<i32>,
    ) -> AssertionResult;

    /// Evaluate a single assertion, with the screen captured after each step.
    ///
    /// This is the step-aware form of [`ExecutionContext::evaluate_assertion`].
    /// `evaluate_assertion` only ever sees the final screen, so a state the
    /// target passes through and then leaves — a palette that is opened and
    /// then dismissed — is not expressible with it. `StepResult::screen`
    /// already records the intermediate screens; this method is what carries
    /// them to the assertion.
    ///
    /// The default body ignores `steps` and forwards to `evaluate_assertion`,
    /// so an implementation written before this method existed keeps compiling
    /// and keeps behaving identically. Overriding it is the opt-in.
    ///
    /// `steps` is `None` when the execution mode did not supply per-step
    /// screens, which is not the same as an empty slice: an assertion that
    /// needs them should report that they were unavailable rather than assume
    /// no steps ran.
    fn evaluate_assertion_with_steps(
        &self,
        recipe: &Recipe,
        assertion: &Value,
        screen: &str,
        raw_output: &str,
        exit_code: Option<i32>,
        steps: Option<&[StepResult]>,
    ) -> AssertionResult {
        let _ = steps;
        self.evaluate_assertion(recipe, assertion, screen, raw_output, exit_code)
    }

    /// Evaluate all assertions (including implicit `expect_exit_code`).
    fn evaluate_assertions(
        &self,
        recipe: &Recipe,
        screen: &str,
        raw_output: &str,
        exit_code: Option<i32>,
    ) -> Vec<AssertionResult> {
        assertions_with_implicit(recipe)
            .iter()
            .map(|a| self.evaluate_assertion(recipe, a, screen, raw_output, exit_code))
            .collect()
    }

    /// Evaluate all assertions, with the screen captured after each step.
    ///
    /// The step-aware form of [`ExecutionContext::evaluate_assertions`]; it
    /// assembles the same assertion list and routes each one through
    /// [`ExecutionContext::evaluate_assertion_with_steps`]. An implementation
    /// that overrides `evaluate_assertions` and wants the override to apply on
    /// this path must override this method too.
    fn evaluate_assertions_with_steps(
        &self,
        recipe: &Recipe,
        screen: &str,
        raw_output: &str,
        exit_code: Option<i32>,
        steps: Option<&[StepResult]>,
    ) -> Vec<AssertionResult> {
        assertions_with_implicit(recipe)
            .iter()
            .map(|a| {
                self.evaluate_assertion_with_steps(recipe, a, screen, raw_output, exit_code, steps)
            })
            .collect()
    }

    /// Return the recipe's timeout as Duration.
    fn recipe_timeout(&self, recipe: &Recipe) -> Duration {
        Duration::from_secs_f64(recipe.timeout_seconds)
    }
}

/// Public execution-mode trait.
///
/// Implementations receive `&mut dyn ExecutionContext` and must not downcast
/// to a concrete runner. This is enforced by taking a trait object.
#[allow(clippy::type_complexity)]
pub trait ExecutionMode: Send + Sync {
    /// Mode name (e.g. "scripted_pty").
    fn name(&self) -> &str;

    /// Execute the recipe, returning steps, assertions, raw output, exit code, and screen.
    fn execute(
        &self,
        ctx: &mut dyn ExecutionContext,
        recipe: &Recipe,
        run_dir: &Path,
    ) -> Result<ExecutionResult, ExecutionError>;
}

/// PTY-based scripted execution mode.
///
/// Uses `ExecutionContext::create_session` and `run_step`; does not touch
/// runner private methods.
pub struct ScriptedPtyMode;

impl ExecutionMode for ScriptedPtyMode {
    fn name(&self) -> &str {
        "scripted_pty"
    }

    fn execute(
        &self,
        ctx: &mut dyn ExecutionContext,
        recipe: &Recipe,
        run_dir: &Path,
    ) -> Result<ExecutionResult, ExecutionError> {
        let cast_path = run_dir.join("session.cast");
        let mut session = ctx.create_session(
            recipe.command.argv.clone(),
            recipe.command.cwd.clone(),
            recipe.command.env.clone(),
            recipe.cols,
            recipe.rows,
            cast_path.clone(),
        )?;
        let mut steps = Vec::new();
        for (i, step) in recipe.steps.iter().enumerate() {
            let result = ctx.run_step(session.as_mut(), step, i + 1);
            let passed = result.passed;
            steps.push(result);
            if !passed {
                break;
            }
        }
        let timeout = ctx.recipe_timeout(recipe);
        if recipe.expect_exit_code.is_some() {
            let _ = session.wait_for_exit(timeout);
        } else {
            let _ = session.wait_for_idle(
                Duration::from_secs_f64(0.5),
                timeout.min(Duration::from_secs(3)),
            );
        }
        let raw_output = session.raw_output().to_string();
        let exit_code = session.exit_code();
        let screen = session.screen().to_string();
        let assertions = ctx.evaluate_assertions_with_steps(
            recipe,
            &screen,
            &raw_output,
            exit_code,
            Some(&steps),
        );
        let _ = session.close();
        Ok((steps, assertions, raw_output, exit_code, screen))
    }
}

/// Non-PTY process mode — same context-bound implementation.
pub struct ScriptedProcessMode;

impl ExecutionMode for ScriptedProcessMode {
    fn name(&self) -> &str {
        "scripted_process"
    }

    fn execute(
        &self,
        ctx: &mut dyn ExecutionContext,
        recipe: &Recipe,
        run_dir: &Path,
    ) -> Result<ExecutionResult, ExecutionError> {
        let cast_path = run_dir.join("session.cast");
        let mut session = ctx.create_session(
            recipe.command.argv.clone(),
            recipe.command.cwd.clone(),
            recipe.command.env.clone(),
            recipe.cols,
            recipe.rows,
            cast_path,
        )?;
        let mut steps = Vec::new();
        for (i, step) in recipe.steps.iter().enumerate() {
            let result = ctx.run_step(session.as_mut(), step, i + 1);
            let passed = result.passed;
            steps.push(result);
            if !passed {
                break;
            }
        }
        let _ = session.wait_for_exit(ctx.recipe_timeout(recipe));
        let raw_output = session.raw_output().to_string();
        let exit_code = session.exit_code();
        let screen = session.screen().to_string();
        let assertions = ctx.evaluate_assertions_with_steps(
            recipe,
            &screen,
            &raw_output,
            exit_code,
            Some(&steps),
        );
        let _ = session.close();
        Ok((steps, assertions, raw_output, exit_code, screen))
    }
}

/// Agent-driven mode — delegates to an agent runner via the context.
///
/// In the Rust port the agent runner is invoked through a dedicated
/// `AgentExecution` hook on the context; this simplified mode captures the
/// PTY path for contract testing. Real subprocess work lives in
/// `AgentDrivenRunner::run` (see `crate::agent`).
pub struct AgentDrivenMode;

impl ExecutionMode for AgentDrivenMode {
    fn name(&self) -> &str {
        "agent_driven"
    }

    fn execute(
        &self,
        ctx: &mut dyn ExecutionContext,
        recipe: &Recipe,
        run_dir: &Path,
    ) -> Result<ExecutionResult, ExecutionError> {
        let cast_path = run_dir.join("session.cast");
        let mut session = ctx.create_session(
            recipe.command.argv.clone(),
            recipe.command.cwd.clone(),
            recipe.command.env.clone(),
            recipe.cols,
            recipe.rows,
            cast_path,
        )?;
        let prompt = crate::agent::build_agent_prompt(recipe);
        let _ = std::fs::write(run_dir.join("agent_prompt.md"), &prompt);
        let raw_output = format!(
            "{{\"assertions\":{{}},\"transcript\":\"agent executed for recipe {}\"}}",
            recipe.name
        );
        let parsed = crate::agent::parse_agent_output(&raw_output);
        let _ = std::fs::write(run_dir.join("agent_transcript.md"), &parsed.transcript);
        let screen = parsed.transcript.clone();
        let checks = if recipe.checks.is_empty() {
            vec!["Codex operator completed the verification".to_string()]
        } else {
            recipe.checks.clone()
        };
        let assertions = checks
            .iter()
            .map(|check| {
                let passed = parsed.assertions.get(check).copied().unwrap_or(false);
                AssertionResult {
                    name: check.clone(),
                    passed,
                    detail: if passed {
                        "agent reported pass".to_string()
                    } else {
                        "agent did not report pass".to_string()
                    },
                }
            })
            .collect();
        let steps = vec![StepResult {
            name: "codex-operator".to_string(),
            passed: true,
            detail: "agent mode via ExecutionContext".to_string(),
            screen: screen.clone(),
        }];
        let _ = session.close();
        Ok((steps, assertions, raw_output, Some(0), screen))
    }
}
