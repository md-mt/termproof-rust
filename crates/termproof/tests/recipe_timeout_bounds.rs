//! `timeout_seconds` at the recipe level is recipe-controlled input too.
//!
//! `specs/001-recipe-format/spec.md` FR-026 puts nothing-may-panic on the load
//! path, and `specs/002-builtin-steps/spec.md` FR-007 says the same of a step's
//! own duration: clamp the deadline, do not take the process down. The recipe's
//! whole-run timeout is the same value from the same file and the same reading
//! applies — `Duration::from_secs_f64` panics on NaN and above roughly `1.8e19`.
//!
//! This was unreachable while nothing executed recipes. It is reachable now.

use std::time::Duration;

use termproof_core::execution::ExecutionContext;
use termproof_core::models::{AssertionResult, Recipe, StepResult};
use termproof_terminal::Session;

/// The smallest context that satisfies the trait; every method under test is a
/// provided one, so the required bodies are never called.
struct Probe;

impl ExecutionContext for Probe {
    fn create_session(
        &mut self,
        _argv: Vec<String>,
        _cwd: Option<String>,
        _env: std::collections::HashMap<String, String>,
        _cols: u16,
        _rows: u16,
        _cast_path: std::path::PathBuf,
    ) -> Result<Box<dyn Session>, termproof_core::execution::ExecutionError> {
        unreachable!("recipe_timeout does not create a session")
    }

    fn run_step(
        &mut self,
        _session: &mut dyn Session,
        _step: &serde_json::Value,
        _index: usize,
    ) -> StepResult {
        unreachable!("recipe_timeout does not run steps")
    }

    fn evaluate_assertion(
        &self,
        _recipe: &Recipe,
        _assertion: &serde_json::Value,
        _screen: &str,
        _raw_output: &str,
        _exit_code: Option<i32>,
    ) -> AssertionResult {
        unreachable!("recipe_timeout does not evaluate assertions")
    }
}

fn recipe_with_timeout(seconds: f64) -> Recipe {
    Recipe {
        timeout_seconds: seconds,
        ..Recipe::default()
    }
}

#[test]
fn a_nan_timeout_is_a_deadline_already_past() {
    assert_eq!(
        Probe.recipe_timeout(&recipe_with_timeout(f64::NAN)),
        Duration::ZERO
    );
}

#[test]
fn a_negative_timeout_is_a_deadline_already_past() {
    assert_eq!(
        Probe.recipe_timeout(&recipe_with_timeout(-1.0)),
        Duration::ZERO
    );
}

#[test]
fn an_enormous_timeout_clamps_instead_of_panicking() {
    let clamped = Probe.recipe_timeout(&recipe_with_timeout(1e300));
    assert!(
        clamped.as_secs() >= 100 * 365 * 24 * 60 * 60,
        "expected a far-future deadline, got {clamped:?}"
    );
}

#[test]
fn an_infinite_timeout_clamps_instead_of_panicking() {
    let clamped = Probe.recipe_timeout(&recipe_with_timeout(f64::INFINITY));
    assert!(
        clamped.as_secs() >= 100 * 365 * 24 * 60 * 60,
        "expected a far-future deadline, got {clamped:?}"
    );
}

#[test]
fn an_ordinary_timeout_is_passed_through() {
    assert_eq!(
        Probe.recipe_timeout(&recipe_with_timeout(2.5)),
        Duration::from_secs_f64(2.5)
    );
}
