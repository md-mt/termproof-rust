//! Contract tests for the step-aware assertion context.
//!
//! Two properties are under test:
//!
//! 1. an assertion can read the screen captured after a named mid-flow step,
//!    including one whose content is absent from the final screen;
//! 2. an `ExecutionContext` written against the signature that existed before
//!    the step-aware methods were added still compiles and still runs, because
//!    the new methods carry default bodies that forward to the old one.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};
use termproof::models::{AssertionResult, CommandSpec, Recipe, StepResult};
use termproof::terminal::{InMemorySession, Session};
use termproof::{ExecutionContext, ExecutionError, ExecutionMode, ScriptedPtyMode};

/// Screens the fake context reports for each step, in order.
const STEP_SCREENS: &[(&str, &str)] = &[
    ("open the palette", "Command palette\n> "),
    ("dismiss the palette", "editor: main.rs"),
];

/// Final screen the session reports — deliberately free of palette text, so an
/// assertion that finds "Command palette" can only have read a step screen.
const FINAL_SCREEN: &str = "editor: main.rs";

fn recipe(assertions: Vec<Value>) -> Recipe {
    Recipe {
        name: "step-context".to_string(),
        command: CommandSpec {
            argv: vec!["true".to_string()],
            ..Default::default()
        },
        steps: STEP_SCREENS
            .iter()
            .map(|(name, _)| json!({"action": "send_line", "name": name, "text": "x"}))
            .collect(),
        assertions,
        expect_exit_code: None,
        ..Default::default()
    }
}

/// The shared session plumbing both fake contexts need.
fn open_session(argv: Vec<String>, cast_path: PathBuf, cols: u16, rows: u16) -> InMemorySession {
    let mut session = InMemorySession::new(argv, cast_path, cols, rows);
    session.set_screen(FINAL_SCREEN);
    session.set_raw(FINAL_SCREEN);
    session
}

fn step_result(index: usize) -> StepResult {
    let (name, screen) = STEP_SCREENS[index - 1];
    StepResult {
        name: name.to_string(),
        passed: true,
        detail: "ok".to_string(),
        screen: screen.to_string(),
    }
}

/// A context that opts into the per-step screens.
///
/// Its `evaluate_assertion_with_steps` implements a `step_screen_contains`
/// assertion: `step` names the `StepResult` whose screen to read.
struct StepAwareContext;

impl ExecutionContext for StepAwareContext {
    fn create_session(
        &mut self,
        argv: Vec<String>,
        _cwd: Option<String>,
        _env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        cast_path: PathBuf,
    ) -> Result<Box<dyn Session>, ExecutionError> {
        Ok(Box::new(open_session(argv, cast_path, cols, rows)))
    }

    fn run_step(&mut self, _session: &mut dyn Session, _step: &Value, index: usize) -> StepResult {
        step_result(index)
    }

    fn evaluate_assertion(
        &self,
        _recipe: &Recipe,
        assertion: &Value,
        screen: &str,
        _raw_output: &str,
        _exit_code: Option<i32>,
    ) -> AssertionResult {
        let value = assertion["value"].as_str().unwrap_or_default();
        AssertionResult {
            name: "screen_contains".to_string(),
            passed: screen.contains(value),
            detail: format!("final screen contains {value:?}"),
        }
    }

    fn evaluate_assertion_with_steps(
        &self,
        recipe: &Recipe,
        assertion: &Value,
        screen: &str,
        raw_output: &str,
        exit_code: Option<i32>,
        steps: Option<&[StepResult]>,
    ) -> AssertionResult {
        if assertion["type"] != json!("step_screen_contains") {
            return self.evaluate_assertion(recipe, assertion, screen, raw_output, exit_code);
        }
        let name = "step_screen_contains".to_string();
        let Some(steps) = steps else {
            return AssertionResult {
                name,
                passed: false,
                detail: "per-step screens were not supplied by the execution mode".to_string(),
            };
        };
        let wanted = assertion["step"].as_str().unwrap_or_default();
        let value = assertion["value"].as_str().unwrap_or_default();
        match steps.iter().find(|s| s.name == wanted) {
            Some(step) => AssertionResult {
                name,
                passed: step.screen.contains(value),
                detail: format!("screen after {wanted:?} contains {value:?}"),
            },
            None => AssertionResult {
                name,
                passed: false,
                detail: format!("no step named {wanted:?}"),
            },
        }
    }
}

/// A context written against the pre-change trait surface, verbatim.
///
/// It implements only `create_session`, `run_step` and `evaluate_assertion` —
/// the three methods that were required before the step-aware pair was added.
/// That this file compiles is the assertion; the test below only exercises it.
struct LegacyContext;

impl ExecutionContext for LegacyContext {
    fn create_session(
        &mut self,
        argv: Vec<String>,
        _cwd: Option<String>,
        _env: HashMap<String, String>,
        cols: u16,
        rows: u16,
        cast_path: PathBuf,
    ) -> Result<Box<dyn Session>, ExecutionError> {
        Ok(Box::new(open_session(argv, cast_path, cols, rows)))
    }

    fn run_step(&mut self, _session: &mut dyn Session, _step: &Value, index: usize) -> StepResult {
        step_result(index)
    }

    fn evaluate_assertion(
        &self,
        _recipe: &Recipe,
        assertion: &Value,
        screen: &str,
        _raw_output: &str,
        _exit_code: Option<i32>,
    ) -> AssertionResult {
        let value = assertion["value"].as_str().unwrap_or_default();
        AssertionResult {
            name: "screen_contains".to_string(),
            passed: screen.contains(value),
            detail: format!("final screen contains {value:?}"),
        }
    }
}

#[test]
fn assertion_reads_a_mid_flow_step_screen() {
    let recipe = recipe(vec![
        json!({
            "type": "step_screen_contains",
            "step": "open the palette",
            "value": "Command palette",
        }),
        json!({"type": "screen_contains", "value": "Command palette"}),
    ]);
    let dir = tempfile::tempdir().expect("tempdir");
    let (_steps, assertions, _raw, _code, screen) = ScriptedPtyMode
        .execute(&mut StepAwareContext, &recipe, dir.path())
        .expect("execute");

    // The palette text is gone by the end of the run, so the step-aware
    // assertion is the only one that can see it.
    assert_eq!(screen, FINAL_SCREEN);
    assert!(!screen.contains("Command palette"));
    assert!(
        assertions[0].passed,
        "step-aware assertion should read the palette step's screen: {:?}",
        assertions[0]
    );
    assert!(
        !assertions[1].passed,
        "final-screen assertion should still fail: {:?}",
        assertions[1]
    );
}

#[test]
fn step_aware_assertion_distinguishes_unsupplied_steps_from_none_matching() {
    let recipe = recipe(vec![json!({
        "type": "step_screen_contains",
        "step": "open the palette",
        "value": "Command palette",
    })]);

    let unsupplied = StepAwareContext.evaluate_assertions_with_steps(
        &recipe,
        FINAL_SCREEN,
        FINAL_SCREEN,
        None,
        None,
    );
    assert!(!unsupplied[0].passed);
    assert!(unsupplied[0].detail.contains("were not supplied"));

    let ran_nothing = StepAwareContext.evaluate_assertions_with_steps(
        &recipe,
        FINAL_SCREEN,
        FINAL_SCREEN,
        None,
        Some(&[]),
    );
    assert!(!ran_nothing[0].passed);
    assert!(ran_nothing[0].detail.contains("no step named"));
}

#[test]
fn old_signature_implementation_still_compiles_and_behaves_identically() {
    let recipe = recipe(vec![json!({"type": "screen_contains", "value": "editor"})]);
    let dir = tempfile::tempdir().expect("tempdir");

    // `ScriptedPtyMode` now calls `evaluate_assertions_with_steps`, which the
    // legacy context does not implement. The default body must forward to its
    // `evaluate_assertion`, giving the same verdicts as the step-blind call.
    let (steps, via_mode, _raw, _code, _screen) = ScriptedPtyMode
        .execute(&mut LegacyContext, &recipe, dir.path())
        .expect("execute");
    let direct = LegacyContext.evaluate_assertions(&recipe, FINAL_SCREEN, FINAL_SCREEN, None);
    let forwarded = LegacyContext.evaluate_assertions_with_steps(
        &recipe,
        FINAL_SCREEN,
        FINAL_SCREEN,
        None,
        Some(&steps),
    );

    assert!(via_mode[0].passed);
    assert_eq!(via_mode, direct);
    assert_eq!(forwarded, direct);
}
