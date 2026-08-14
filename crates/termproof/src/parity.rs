//! Compare two verification runs and report where they disagree.
//!
//! Useful whenever two things are supposed to produce the same verdicts and you
//! need that checked rather than assumed:
//!
//! - two implementations behind a switch — "switchable" is a claim until
//!   something compares them;
//! - a release candidate against the last known-good run;
//! - the same recipes before and after a refactor.
//!
//! It works on [`RunResult`] values, so it does not care what produced them.
//! Foreign payloads — output from another implementation, or from an older
//! version of this one — can be read with [`summarize_json`], which tolerates a
//! shape that has drifted rather than refusing it.
//!
//! # Verdict, score, detail
//!
//! Three grades of disagreement, reported separately because they mean
//! different things:
//!
//! | | meaning |
//! |---|---|
//! | [`Divergence::Verdict`] | the two disagree on pass/fail — always a real difference |
//! | [`Divergence::Score`] | same verdict, different partial credit — a pass/fail comparison would hide it |
//! | [`Divergence::Detail`] | same verdict, different wording — usually cosmetic, occasionally a clue |
//!
//! Separating detail from verdict matters when comparing implementations that
//! word their diagnostics independently: hundreds of harmless wording
//! differences would otherwise bury the handful of real ones.
//!
//! # It refuses rather than guesses
//!
//! An empty payload is an error, not an empty divergence list. A comparison
//! that reports "no differences" because it read nothing is worse than no
//! comparison, because it looks like success.
//!
//! ```
//! use termproof::parity::{compare, summarize, Divergence};
//! # use termproof::result::RunResult;
//! # use std::collections::BTreeMap;
//! # fn run(name: &str, passed: bool) -> RunResult {
//! #     RunResult { result_version: Some(1), recipe_name: name.into(), passed, exit_code: None,
//! #         duration_seconds: 0.0, priority: "P0".into(), execution: "scripted".into(),
//! #         renderer: "default".into(), score: if passed { 1.0 } else { 0.0 },
//! #         steps: vec![], assertions: vec![], artifacts: BTreeMap::new() }
//! # }
//! let before = summarize(&[run("login", true)]);
//! let after = summarize(&[run("login", false)]);
//!
//! let divergences = compare(&before, &after, "before", "after")?;
//! assert!(matches!(divergences[0], Divergence::Verdict { .. }));
//! # Ok::<(), termproof::parity::ParityError>(())
//! ```

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use thiserror::Error;

use crate::result::RunResult;

/// One recipe's outcome, reduced to what a comparison needs.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeOutcome {
    /// Whether the recipe passed overall.
    pub passed: bool,
    /// Score in `0.0..=1.0`.
    pub score: f64,
    /// Assertion name → (passed, detail).
    pub assertions: BTreeMap<String, (bool, String)>,
}

/// A run reduced to the parts a comparison looks at.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RunSummary {
    /// Recipe key → outcome. Keyed by `recipe_name:renderer` when a renderer is
    /// present, so one recipe run under two renderers stays two entries rather
    /// than one overwriting the other.
    pub recipes: BTreeMap<String, RecipeOutcome>,
}

/// How two runs differ on one recipe.
#[derive(Debug, Clone, PartialEq)]
pub enum Divergence {
    /// One side ran it and the other did not.
    OnlyIn {
        /// Recipe key.
        recipe: String,
        /// Name of the side that ran it.
        side: String,
    },
    /// Both ran it and disagreed on pass/fail.
    Verdict {
        /// Recipe key.
        recipe: String,
        /// Left side's verdict.
        left: bool,
        /// Right side's verdict.
        right: bool,
    },
    /// Same verdict, different score.
    Score {
        /// Recipe key.
        recipe: String,
        /// Left side's score.
        left: f64,
        /// Right side's score.
        right: f64,
    },
    /// Assertions that disagree on pass/fail, or exist on only one side.
    Assertions {
        /// Recipe key.
        recipe: String,
        /// Assertions present on both sides with different verdicts.
        differing: Vec<String>,
        /// Assertions only the left side reported.
        only_left: Vec<String>,
        /// Assertions only the right side reported.
        only_right: Vec<String>,
    },
    /// Assertions that agree on pass/fail but word their detail differently.
    ///
    /// Reported separately from [`Divergence::Assertions`] so a wording change
    /// cannot be mistaken for a behaviour change. See [`compare_detail`].
    Detail {
        /// Recipe key.
        recipe: String,
        /// Assertions whose `detail` differs.
        differing: Vec<String>,
    },
}

/// Why a comparison could not be made at all.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParityError {
    /// A side had no recipes, which is never a legitimate run.
    #[error("`{side}` contained no recipes; refusing to report agreement")]
    Empty {
        /// Name of the empty side.
        side: String,
    },
}

/// Whether [`compare`] reports detail-only differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailMode {
    /// Ignore `detail`; compare verdicts only. The default, because two
    /// implementations that word diagnostics independently produce many
    /// harmless differences.
    #[default]
    Ignore,
    /// Report `detail` differences as [`Divergence::Detail`].
    Compare,
}

/// The key a recipe is compared under.
fn key(result: &RunResult) -> String {
    if result.renderer.is_empty() {
        result.recipe_name.clone()
    } else {
        format!("{}:{}", result.recipe_name, result.renderer)
    }
}

/// Reduce typed results to a comparable summary.
pub fn summarize(results: &[RunResult]) -> RunSummary {
    let mut recipes = BTreeMap::new();
    for r in results {
        recipes.insert(
            key(r),
            RecipeOutcome {
                passed: r.passed,
                score: r.score,
                assertions: r
                    .assertions
                    .iter()
                    .map(|a| (a.name.clone(), (a.passed, a.detail.clone())))
                    .collect(),
            },
        );
    }
    RunSummary { recipes }
}

/// Reduce a foreign JSON payload to a comparable summary.
///
/// Deliberately lenient about shape: a payload from another implementation, or
/// from a version that predates a field, should still be comparable. Anything
/// missing reads as absent rather than as a parse failure — an entry with no
/// recognisable `recipe_name` is skipped, and a run with no recognisable
/// entries yields an empty summary, which [`compare`] then rejects loudly.
///
/// It does **not** gate on [`crate::result::RESULT_SCHEMA_VERSION`], on purpose.
/// Every field it reads — `recipe_name`, `renderer`, `passed`, `score`,
/// `assertions` — is present in version 1 and is what a foreign writer emits
/// anyway, so a version check here would buy nothing and cost the exact failure
/// the version scheme exists to avoid: a comparison refusing to run because the
/// other side is not this build. Readers that need the whole typed payload go
/// through [`crate::result::RunResult::from_json_value`], which does check.
pub fn summarize_json(payload: &serde_json::Value) -> RunSummary {
    let mut recipes = BTreeMap::new();
    let Some(results) = payload.get("results").and_then(|r| r.as_array()) else {
        return RunSummary { recipes };
    };
    for r in results {
        let Some(name) = r.get("recipe_name").and_then(|n| n.as_str()) else {
            continue;
        };
        let renderer = r.get("renderer").and_then(|v| v.as_str()).unwrap_or("");
        let entry_key = if renderer.is_empty() {
            name.to_string()
        } else {
            format!("{name}:{renderer}")
        };
        recipes.insert(
            entry_key,
            RecipeOutcome {
                passed: r.get("passed").and_then(|p| p.as_bool()).unwrap_or(false),
                score: r.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0),
                assertions: json_assertions(r.get("assertions")),
            },
        );
    }
    RunSummary { recipes }
}

/// Read assertions from either shape a payload might use.
///
/// A list of `{name, passed, detail}` is this crate's own format. A bare
/// `{name: bool}` map is what a simpler implementation tends to emit, and
/// dropping those on the floor would make the comparison silently shallower
/// rather than visibly unsupported.
fn json_assertions(value: Option<&serde_json::Value>) -> BTreeMap<String, (bool, String)> {
    let mut out = BTreeMap::new();
    match value {
        Some(serde_json::Value::Array(items)) => {
            for a in items {
                if let Some(name) = a.get("name").and_then(|n| n.as_str()) {
                    out.insert(
                        name.to_string(),
                        (
                            a.get("passed").and_then(|p| p.as_bool()).unwrap_or(false),
                            a.get("detail")
                                .and_then(|d| d.as_str())
                                .unwrap_or_default()
                                .to_string(),
                        ),
                    );
                }
            }
        }
        Some(serde_json::Value::Object(map)) => {
            for (name, v) in map {
                if let Some(passed) = v.as_bool() {
                    out.insert(name.clone(), (passed, String::new()));
                }
            }
        }
        _ => {}
    }
    out
}

/// Every way `left` and `right` disagree, ignoring diagnostic wording.
pub fn compare(
    left: &RunSummary,
    right: &RunSummary,
    left_name: &str,
    right_name: &str,
) -> Result<Vec<Divergence>, ParityError> {
    compare_with(left, right, left_name, right_name, DetailMode::Ignore)
}

/// [`compare`], also reporting assertions whose `detail` differs.
pub fn compare_detail(
    left: &RunSummary,
    right: &RunSummary,
    left_name: &str,
    right_name: &str,
) -> Result<Vec<Divergence>, ParityError> {
    compare_with(left, right, left_name, right_name, DetailMode::Compare)
}

/// Every way `left` and `right` disagree.
///
/// Refuses rather than guesses: an empty payload is an error, not an empty
/// divergence list.
pub fn compare_with(
    left: &RunSummary,
    right: &RunSummary,
    left_name: &str,
    right_name: &str,
    detail: DetailMode,
) -> Result<Vec<Divergence>, ParityError> {
    for (side, run) in [(left_name, left), (right_name, right)] {
        if run.recipes.is_empty() {
            return Err(ParityError::Empty {
                side: side.to_string(),
            });
        }
    }

    let mut out = Vec::new();
    let names: BTreeSet<&String> = left.recipes.keys().chain(right.recipes.keys()).collect();
    for name in names {
        match (left.recipes.get(name), right.recipes.get(name)) {
            (Some(l), Some(r)) => compare_one(name, l, r, detail, &mut out),
            (Some(_), None) => out.push(Divergence::OnlyIn {
                recipe: name.clone(),
                side: left_name.to_string(),
            }),
            (None, Some(_)) => out.push(Divergence::OnlyIn {
                recipe: name.clone(),
                side: right_name.to_string(),
            }),
            (None, None) => unreachable!("name came from one of the two maps"),
        }
    }
    Ok(out)
}

fn compare_one(
    name: &str,
    l: &RecipeOutcome,
    r: &RecipeOutcome,
    detail: DetailMode,
    out: &mut Vec<Divergence>,
) {
    if l.passed != r.passed {
        out.push(Divergence::Verdict {
            recipe: name.to_string(),
            left: l.passed,
            right: r.passed,
        });
    } else if (l.score - r.score).abs() > f64::EPSILON {
        out.push(Divergence::Score {
            recipe: name.to_string(),
            left: l.score,
            right: r.score,
        });
    }

    let lk: BTreeSet<&String> = l.assertions.keys().collect();
    let rk: BTreeSet<&String> = r.assertions.keys().collect();
    let shared: Vec<String> = lk.intersection(&rk).map(|k| (*k).clone()).collect();

    let differing: Vec<String> = shared
        .iter()
        .filter(|k| l.assertions[*k].0 != r.assertions[*k].0)
        .cloned()
        .collect();
    let only_left: Vec<String> = lk.difference(&rk).map(|k| (*k).clone()).collect();
    let only_right: Vec<String> = rk.difference(&lk).map(|k| (*k).clone()).collect();

    if !differing.is_empty() || !only_left.is_empty() || !only_right.is_empty() {
        out.push(Divergence::Assertions {
            recipe: name.to_string(),
            differing,
            only_left,
            only_right,
        });
    }

    if detail == DetailMode::Compare {
        let worded: Vec<String> = shared
            .iter()
            .filter(|k| {
                let (lp, ld) = &l.assertions[*k];
                let (rp, rd) = &r.assertions[*k];
                lp == rp && ld != rd
            })
            .cloned()
            .collect();
        if !worded.is_empty() {
            out.push(Divergence::Detail {
                recipe: name.to_string(),
                differing: worded,
            });
        }
    }
}

/// Render divergences as markdown.
pub fn render(divergences: &[Divergence], left_name: &str, right_name: &str) -> String {
    let mut out = vec!["# Parity report".to_string(), String::new()];
    if divergences.is_empty() {
        out.push(format!(
            "`{left_name}` and `{right_name}` agree on every recipe."
        ));
        out.push(String::new());
        return out.join("\n");
    }
    out.push(format!(
        "**{} divergence(s)** between `{left_name}` and `{right_name}`.",
        divergences.len()
    ));
    out.push(String::new());
    for d in divergences {
        out.push(render_one(d, left_name, right_name));
    }
    out.push(String::new());
    out.join("\n")
}

fn render_one(d: &Divergence, left_name: &str, right_name: &str) -> String {
    match d {
        Divergence::OnlyIn { recipe, side } => format!("- `{recipe}` — ran only in `{side}`"),
        Divergence::Verdict {
            recipe,
            left,
            right,
        } => format!(
            "- `{recipe}` — **verdict differs**: `{left_name}` {}, `{right_name}` {}",
            verdict(*left),
            verdict(*right)
        ),
        Divergence::Score {
            recipe,
            left,
            right,
        } => format!("- `{recipe}` — same verdict, score {left:.2} vs {right:.2}"),
        Divergence::Assertions {
            recipe,
            differing,
            only_left,
            only_right,
        } => {
            let mut s = format!("- `{recipe}` — assertion differences:\n");
            if !differing.is_empty() {
                s.push_str(&format!("  - disagree: {}\n", backticked(differing)));
            }
            if !only_left.is_empty() {
                s.push_str(&format!(
                    "  - only in `{left_name}`: {}\n",
                    backticked(only_left)
                ));
            }
            if !only_right.is_empty() {
                s.push_str(&format!(
                    "  - only in `{right_name}`: {}\n",
                    backticked(only_right)
                ));
            }
            s.trim_end().to_string()
        }
        Divergence::Detail { recipe, differing } => format!(
            "- `{recipe}` — same verdicts, different wording: {}",
            backticked(differing)
        ),
    }
}

fn backticked(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn verdict(passed: bool) -> &'static str {
    if passed {
        "PASS"
    } else {
        "FAIL"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::AssertionResult;

    fn assertion(name: &str, passed: bool, detail: &str) -> AssertionResult {
        AssertionResult {
            name: name.to_string(),
            passed,
            detail: detail.to_string(),
        }
    }

    fn run(name: &str, passed: bool, score: f64, assertions: Vec<AssertionResult>) -> RunResult {
        RunResult {
            result_version: Some(crate::result::RESULT_SCHEMA_VERSION),
            recipe_name: name.to_string(),
            passed,
            exit_code: None,
            duration_seconds: 0.0,
            priority: "P0".to_string(),
            execution: "scripted".to_string(),
            renderer: String::new(),
            score,
            steps: vec![],
            assertions,
            artifacts: BTreeMap::new(),
        }
    }

    #[test]
    fn identical_runs_agree() {
        let a = summarize(&[run("login", true, 1.0, vec![assertion("ok", true, "fine")])]);
        assert!(compare(&a, &a, "left", "right").unwrap().is_empty());
    }

    #[test]
    fn an_empty_side_is_an_error_not_agreement() {
        // The failure this exists to prevent: a gate that goes green because it
        // compared nothing looks exactly like a gate that passed.
        let full = summarize(&[run("login", true, 1.0, vec![])]);
        let empty = RunSummary::default();
        assert_eq!(
            compare(&full, &empty, "left", "right"),
            Err(ParityError::Empty {
                side: "right".to_string()
            })
        );
        assert_eq!(
            compare(&empty, &full, "left", "right"),
            Err(ParityError::Empty {
                side: "left".to_string()
            })
        );
    }

    #[test]
    fn a_differing_verdict_is_reported() {
        let l = summarize(&[run("login", true, 1.0, vec![])]);
        let r = summarize(&[run("login", false, 0.0, vec![])]);
        assert_eq!(
            compare(&l, &r, "a", "b").unwrap(),
            vec![Divergence::Verdict {
                recipe: "login".to_string(),
                left: true,
                right: false
            }]
        );
    }

    #[test]
    fn a_score_difference_survives_an_agreeing_verdict() {
        // Both failed, but one got half credit. A pass/fail comparison would
        // call this agreement.
        let l = summarize(&[run("login", false, 0.5, vec![])]);
        let r = summarize(&[run("login", false, 0.0, vec![])]);
        assert!(matches!(
            compare(&l, &r, "a", "b").unwrap()[0],
            Divergence::Score { .. }
        ));
    }

    #[test]
    fn assertion_level_differences_are_named() {
        let l = summarize(&[run(
            "login",
            true,
            1.0,
            vec![assertion("shared", true, ""), assertion("only_l", true, "")],
        )]);
        let r = summarize(&[run(
            "login",
            true,
            1.0,
            vec![
                assertion("shared", false, ""),
                assertion("only_r", true, ""),
            ],
        )]);
        let d = compare(&l, &r, "a", "b").unwrap();
        let found = d
            .iter()
            .find_map(|x| match x {
                Divergence::Assertions {
                    differing,
                    only_left,
                    only_right,
                    ..
                } => Some((differing.clone(), only_left.clone(), only_right.clone())),
                _ => None,
            })
            .expect("assertion divergence");
        assert_eq!(found.0, vec!["shared".to_string()]);
        assert_eq!(found.1, vec!["only_l".to_string()]);
        assert_eq!(found.2, vec!["only_r".to_string()]);
    }

    #[test]
    fn wording_is_ignored_by_default() {
        // Two implementations word diagnostics independently. If every wording
        // difference counted, the real divergences would be buried.
        let l = summarize(&[run(
            "login",
            true,
            1.0,
            vec![assertion("ok", true, "matched at line 3")],
        )]);
        let r = summarize(&[run(
            "login",
            true,
            1.0,
            vec![assertion("ok", true, "found on row 3")],
        )]);
        assert!(compare(&l, &r, "a", "b").unwrap().is_empty());
    }

    #[test]
    fn wording_is_reported_when_asked_for() {
        let l = summarize(&[run("login", true, 1.0, vec![assertion("ok", true, "one")])]);
        let r = summarize(&[run("login", true, 1.0, vec![assertion("ok", true, "two")])]);
        assert_eq!(
            compare_detail(&l, &r, "a", "b").unwrap(),
            vec![Divergence::Detail {
                recipe: "login".to_string(),
                differing: vec!["ok".to_string()]
            }]
        );
    }

    #[test]
    fn one_recipe_under_two_renderers_stays_two_entries() {
        // Keyed by name alone, the second would overwrite the first and a whole
        // renderer's results would vanish without a word.
        let mut a = run("login", true, 1.0, vec![]);
        a.renderer = "alpha".to_string();
        let mut b = run("login", false, 0.0, vec![]);
        b.renderer = "beta".to_string();
        let s = summarize(&[a, b]);
        assert_eq!(s.recipes.len(), 2);
        assert!(s.recipes.contains_key("login:alpha"));
        assert!(s.recipes.contains_key("login:beta"));
    }

    #[test]
    fn a_recipe_only_one_side_ran_is_reported() {
        let l = summarize(&[
            run("login", true, 1.0, vec![]),
            run("logout", true, 1.0, vec![]),
        ]);
        let r = summarize(&[run("login", true, 1.0, vec![])]);
        assert_eq!(
            compare(&l, &r, "a", "b").unwrap(),
            vec![Divergence::OnlyIn {
                recipe: "logout".to_string(),
                side: "a".to_string()
            }]
        );
    }

    #[test]
    fn a_foreign_payload_with_list_assertions_is_readable() {
        let payload = serde_json::json!({
            "results": [{
                "recipe_name": "login", "renderer": "default",
                "passed": true, "score": 1.0,
                "assertions": [{"name": "ok", "passed": true, "detail": "d"}]
            }]
        });
        let s = summarize_json(&payload);
        let o = &s.recipes["login:default"];
        assert!(o.passed);
        assert_eq!(o.assertions["ok"], (true, "d".to_string()));
    }

    #[test]
    fn a_foreign_payload_with_map_assertions_is_readable() {
        // A simpler implementation emits `{name: bool}`. Dropping those would
        // make the comparison silently shallower rather than visibly
        // unsupported.
        let payload = serde_json::json!({
            "results": [{
                "recipe_name": "login",
                "passed": false, "score": 0.5,
                "assertions": {"a": true, "b": false}
            }]
        });
        let s = summarize_json(&payload);
        let o = &s.recipes["login"];
        assert_eq!(o.assertions["a"], (true, String::new()));
        assert_eq!(o.assertions["b"], (false, String::new()));
    }

    #[test]
    fn an_unrecognisable_payload_summarises_to_nothing_and_compare_refuses() {
        // Leniency in the reader must not become leniency in the verdict.
        let s = summarize_json(&serde_json::json!({"unexpected": true}));
        assert!(s.recipes.is_empty());
        let full = summarize(&[run("login", true, 1.0, vec![])]);
        assert!(compare(&full, &s, "a", "b").is_err());
    }

    #[test]
    fn agreement_renders_as_a_sentence_not_an_empty_list() {
        let out = render(&[], "left", "right");
        assert!(out.contains("agree on every recipe"), "{out}");
    }

    #[test]
    fn every_divergence_kind_renders() {
        let all = vec![
            Divergence::OnlyIn {
                recipe: "a".into(),
                side: "left".into(),
            },
            Divergence::Verdict {
                recipe: "b".into(),
                left: true,
                right: false,
            },
            Divergence::Score {
                recipe: "c".into(),
                left: 1.0,
                right: 0.5,
            },
            Divergence::Assertions {
                recipe: "d".into(),
                differing: vec!["x".into()],
                only_left: vec!["y".into()],
                only_right: vec!["z".into()],
            },
            Divergence::Detail {
                recipe: "e".into(),
                differing: vec!["w".into()],
            },
        ];
        let out = render(&all, "left", "right");
        for expected in [
            "ran only in",
            "verdict differs",
            "score",
            "disagree",
            "wording",
        ] {
            assert!(out.contains(expected), "{expected} missing from:\n{out}");
        }
        assert!(out.contains("**5 divergence(s)**"), "{out}");
    }
}
