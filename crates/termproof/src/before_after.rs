//! What changed between two runs of the same suite.
//!
//! Where [`crate::parity`] asks "do these two agree?", this asks the narrower
//! and more actionable question: **which outcomes flipped?**
//!
//! The usual shape is a change under review verified twice — once against a
//! baseline build, once against the candidate — so a reviewer sees exactly
//! which behaviours the change altered rather than a wall of results they have
//! to diff by eye.
//!
//! A recipe present in only one pass is reported as `SKIP` on the missing side.
//! That is a real signal, not a gap to hide: a recipe that stopped running is
//! usually more interesting than one that changed verdict, and silently
//! omitting it would make a shrinking suite look like a stable one.
//!
//! ```
//! use termproof::before_after::build_before_after;
//! # use termproof::result::RunResult;
//! # use std::collections::BTreeMap;
//! # fn run(name: &str, passed: bool) -> RunResult {
//! #     RunResult { recipe_name: name.into(), passed, exit_code: None,
//! #         duration_seconds: 0.0, priority: "P0".into(), execution: "scripted".into(),
//! #         renderer: "default".into(), score: 0.0, steps: vec![], assertions: vec![],
//! #         artifacts: BTreeMap::new() }
//! # }
//! let result = build_before_after(
//!     vec![run("login", true), run("search", true)],
//!     vec![run("login", false), run("search", true)],
//! );
//!
//! assert_eq!(result.deltas.len(), 1);
//! assert_eq!(result.deltas[0].explanation(), "login [default]: PASS -> FAIL");
//! ```

use crate::result::RunResult;

/// Outcome label for a run that happened.
fn outcome(result: &RunResult) -> &'static str {
    if result.passed {
        PASS
    } else {
        FAIL
    }
}

/// The run passed.
pub const PASS: &str = "PASS";
/// The run failed.
pub const FAIL: &str = "FAIL";
/// The recipe did not run on this side.
pub const SKIP: &str = "SKIP";

/// Results are matched on this pair.
fn key(result: &RunResult) -> (String, String) {
    (result.recipe_name.clone(), result.renderer.clone())
}

/// One recipe/renderer whose outcome changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BehaviorDelta {
    /// Recipe name.
    pub recipe: String,
    /// Renderer the recipe ran under.
    pub renderer: String,
    /// [`PASS`], [`FAIL`] or [`SKIP`] before.
    pub before_outcome: String,
    /// [`PASS`], [`FAIL`] or [`SKIP`] after.
    pub after_outcome: String,
}

impl BehaviorDelta {
    /// One line a reviewer can read without context.
    pub fn explanation(&self) -> String {
        format!(
            "{} [{}]: {} -> {}",
            self.recipe, self.renderer, self.before_outcome, self.after_outcome
        )
    }
}

/// Two runs and what changed between them.
#[derive(Debug, Clone)]
pub struct BeforeAfterResult {
    /// The baseline run.
    pub before: Vec<RunResult>,
    /// The candidate run.
    pub after: Vec<RunResult>,
    /// Outcomes that differ, in `before` order then new arrivals.
    pub deltas: Vec<BehaviorDelta>,
}

impl BeforeAfterResult {
    /// Render the deltas as markdown.
    ///
    /// States "none" explicitly rather than emitting nothing: an empty section
    /// and a missing section look identical in a report, and only one of them
    /// means the comparison ran.
    pub fn to_markdown(&self) -> String {
        if self.deltas.is_empty() {
            return "**Behavioral deltas:** none — before/after outcomes match.\n".to_string();
        }
        let mut lines = vec!["**Behavioral deltas:**".to_string(), String::new()];
        for delta in &self.deltas {
            lines.push(format!("- {}", delta.explanation()));
        }
        format!("{}\n", lines.join("\n"))
    }
}

/// The outcome changes from `before` to `after`.
///
/// Matched by `(recipe_name, renderer)`. Ordering follows `before`, with
/// recipes that appear only in `after` appended — so a report reads in a stable
/// order rather than a hash order.
pub fn compute_deltas(before: &[RunResult], after: &[RunResult]) -> Vec<BehaviorDelta> {
    let before_keys: Vec<(String, String)> = before.iter().map(key).collect();
    let after_keys: Vec<(String, String)> = after.iter().map(key).collect();

    let mut ordered_keys: Vec<(String, String)> = before_keys.clone();
    for k in &after_keys {
        if !before_keys.contains(k) {
            ordered_keys.push(k.clone());
        }
    }

    let mut deltas = Vec::new();
    for k in ordered_keys {
        let b = before.iter().find(|r| key(r) == k);
        let a = after.iter().find(|r| key(r) == k);
        let before_outcome = b.map(outcome).unwrap_or(SKIP);
        let after_outcome = a.map(outcome).unwrap_or(SKIP);
        if before_outcome != after_outcome {
            deltas.push(BehaviorDelta {
                recipe: k.0.clone(),
                renderer: k.1.clone(),
                before_outcome: before_outcome.to_string(),
                after_outcome: after_outcome.to_string(),
            });
        }
    }
    deltas
}

/// Assemble a [`BeforeAfterResult`] with its deltas computed.
pub fn build_before_after(before: Vec<RunResult>, after: Vec<RunResult>) -> BeforeAfterResult {
    let deltas = compute_deltas(&before, &after);
    BeforeAfterResult {
        before,
        after,
        deltas,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn run(name: &str, renderer: &str, passed: bool) -> RunResult {
        RunResult {
            recipe_name: name.to_string(),
            passed,
            exit_code: None,
            duration_seconds: 0.0,
            priority: "P0".to_string(),
            execution: "scripted".to_string(),
            renderer: renderer.to_string(),
            score: if passed { 1.0 } else { 0.0 },
            steps: vec![],
            assertions: vec![],
            artifacts: BTreeMap::new(),
        }
    }

    #[test]
    fn identical_runs_have_no_deltas() {
        let a = vec![run("login", "default", true)];
        assert!(compute_deltas(&a, &a).is_empty());
    }

    #[test]
    fn a_regression_is_reported() {
        let deltas = compute_deltas(
            &[run("login", "default", true)],
            &[run("login", "default", false)],
        );
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].explanation(), "login [default]: PASS -> FAIL");
    }

    #[test]
    fn a_fix_is_reported_too() {
        // Not only regressions: a change that fixes something is a behavioural
        // delta a reviewer wants to see confirmed.
        let deltas = compute_deltas(
            &[run("login", "default", false)],
            &[run("login", "default", true)],
        );
        assert_eq!(deltas[0].explanation(), "login [default]: FAIL -> PASS");
    }

    #[test]
    fn the_same_recipe_under_two_renderers_is_two_entries() {
        let deltas = compute_deltas(
            &[run("login", "alpha", true), run("login", "beta", true)],
            &[run("login", "alpha", true), run("login", "beta", false)],
        );
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].renderer, "beta");
    }

    #[test]
    fn a_recipe_that_stopped_running_is_a_delta_not_a_gap() {
        // Silently dropping it would make a shrinking suite look stable.
        let deltas = compute_deltas(&[run("login", "default", true)], &[]);
        assert_eq!(deltas[0].explanation(), "login [default]: PASS -> SKIP");
    }

    #[test]
    fn a_newly_added_recipe_is_reported() {
        let deltas = compute_deltas(&[], &[run("login", "default", true)]);
        assert_eq!(deltas[0].explanation(), "login [default]: SKIP -> PASS");
    }

    #[test]
    fn ordering_follows_before_then_new_arrivals() {
        // A report that reshuffles between runs is hard to read and harder to
        // diff.
        let deltas = compute_deltas(
            &[run("a", "d", true), run("b", "d", true)],
            &[
                run("b", "d", false),
                run("a", "d", false),
                run("c", "d", true),
            ],
        );
        let names: Vec<&str> = deltas.iter().map(|d| d.recipe.as_str()).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn no_deltas_says_so_rather_than_rendering_nothing() {
        // An empty section and a missing section look the same in a report, and
        // only one of them means the comparison ran.
        let r = build_before_after(
            vec![run("login", "default", true)],
            vec![run("login", "default", true)],
        );
        assert!(r.to_markdown().contains("none"), "{}", r.to_markdown());
    }

    #[test]
    fn markdown_lists_every_delta() {
        let r = build_before_after(
            vec![run("a", "d", true), run("b", "d", true)],
            vec![run("a", "d", false), run("b", "d", false)],
        );
        let md = r.to_markdown();
        assert!(md.contains("- a [d]: PASS -> FAIL"), "{md}");
        assert!(md.contains("- b [d]: PASS -> FAIL"), "{md}");
    }

    #[test]
    fn build_keeps_both_sides_alongside_the_deltas() {
        let r = build_before_after(vec![run("a", "d", true)], vec![run("a", "d", false)]);
        assert_eq!(r.before.len(), 1);
        assert_eq!(r.after.len(), 1);
        assert_eq!(r.deltas.len(), 1);
    }
}
