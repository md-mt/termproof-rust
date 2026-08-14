//! The port half of the assertion differential harness.
//!
//! Replays `harness/corpus/assertion_cases.json` — whose expectations were
//! recorded from the Python implementation by `harness/probe_assertions.py` —
//! through the Rust assertions, and reports how many cases agree. See
//! `harness/README.md` for what the corpus does and does not measure.
//!
//! Each case runs on its own thread behind a wall-clock budget, so a case that
//! panics or never returns is recorded rather than taking the measurement down
//! with it.
//!
//! Eighteen cases have no oracle verdict at all: the Python implementation
//! raises out of `evaluate_assertions` and discards every result already
//! collected. `specs/003-builtin-assertions/spec.md` FR-020 supersedes the
//! oracle there, so those are asserted to be *contained* — a returned failing
//! result — rather than compared.
//!
//! Run it with output:
//!
//! ```sh
//! cargo test -p termproof --test differential_assertions -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::Value as JsonValue;

use termproof::assertions;
use termproof::models::{AssertionResult, CommandSpec, Recipe};

/// The measurement, locked in as a ratchet. Raising the floor is the point of
/// the harness; lowering it is a regression and needs saying out loud in the
/// change that does it.
const AGREEMENT_FLOOR: usize = 124;
/// Cases agreeing on pass/fail, whatever the detail says. A fix that corrects a
/// verdict but leaves the wording to a later commit moves this and not the
/// floor above, so it still has to move a number.
const VERDICT_FLOOR: usize = 143;
/// Cases the oracle cannot answer because they end its run. FR-020 requires the
/// port to contain every one, so this is asserted exactly rather than ratcheted.
const CONTAINED_EXACT: usize = 18;

/// Wall-clock budget per case. Every case is a pure function of its inputs plus
/// a `stat`; anything past this is not slow, it is stuck.
const CASE_BUDGET: Duration = Duration::from_secs(10);

/// The fixture-root placeholder, substituted in before a case runs and back out
/// of the result, so an absolute path in a detail is comparable across machines.
const FIXTURE_TOKEN: &str = "@FX";
const DEFAULT_SCREEN: &str = "SCREEN text";
const DEFAULT_RAW: &str = "RAW output";

/// Populated by the panic hook so a panicking case is reported with its message
/// rather than as an anonymous death. Cases run one at a time.
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Materialise the corpus's fixture tree, the same way `probe_assertions.py`
/// does: `null` is a directory, `@hex:...` is those raw bytes.
fn build_fixtures(root: &Path, spec: &JsonValue) {
    for (relative, content) in spec.as_object().expect("fixtures object") {
        let target = root.join(relative);
        let Some(text) = content.as_str() else {
            std::fs::create_dir_all(&target).expect("fixture directory");
            continue;
        };
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("fixture parent");
        }
        match text.strip_prefix("@hex:") {
            Some(hex) => {
                let bytes: Vec<u8> = (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture hex"))
                    .collect();
                std::fs::write(&target, bytes).expect("fixture bytes");
            }
            None => std::fs::write(&target, text).expect("fixture text"),
        }
    }
}

fn substitute(value: &JsonValue, root: &str) -> JsonValue {
    match value {
        JsonValue::String(s) => JsonValue::String(s.replace(FIXTURE_TOKEN, root)),
        JsonValue::Object(map) => JsonValue::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), substitute(v, root)))
                .collect(),
        ),
        JsonValue::Array(items) => {
            JsonValue::Array(items.iter().map(|v| substitute(v, root)).collect())
        }
        _ => value.clone(),
    }
}

fn redact(text: &str, root: &str) -> String {
    text.replace(root, FIXTURE_TOKEN)
}

/// An `exit_code`-shaped corpus field: absent means `0`, explicit null means
/// the process produced no code.
fn optional_code(case: &JsonValue, key: &str) -> Option<i32> {
    match case.get(key) {
        None => Some(0),
        Some(JsonValue::Null) => None,
        Some(value) => Some(value.as_i64().expect("integer exit code") as i32),
    }
}

fn recipe_for(case: &JsonValue, root: &str) -> Recipe {
    let cwd = match case.get("cwd") {
        None => Some(root.to_string()),
        Some(JsonValue::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .expect("cwd string")
                .replace(FIXTURE_TOKEN, root),
        ),
    };
    Recipe {
        name: case["id"].as_str().unwrap_or_default().to_string(),
        command: CommandSpec {
            argv: vec!["true".to_string()],
            cwd,
            ..CommandSpec::default()
        },
        assertions: match case.get("assertions") {
            Some(list) => substitute(list, root)
                .as_array()
                .cloned()
                .unwrap_or_default(),
            None => vec![],
        },
        expect_exit_code: optional_code(case, "expect_exit_code"),
        ..Recipe::default()
    }
}

/// What the port did with one case.
enum Outcome {
    Returned(Vec<AssertionResult>),
    Panicked(String),
    Stuck,
}

fn run_case(case: &JsonValue, root: &str) -> Outcome {
    let recipe = recipe_for(case, root);
    let screen = case
        .get("screen")
        .and_then(JsonValue::as_str)
        .unwrap_or(DEFAULT_SCREEN)
        .to_string();
    let raw_output = case
        .get("raw_output")
        .and_then(JsonValue::as_str)
        .unwrap_or(DEFAULT_RAW)
        .to_string();
    let exit_code = optional_code(case, "exit_code");
    let is_run = case.get("kind").and_then(JsonValue::as_str) == Some("run");
    let assertion = case.get("assertion").map(|a| substitute(a, root));

    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().spawn(move || {
        let results = if is_run {
            assertions::evaluate_all(&recipe, &screen, &raw_output, exit_code)
        } else {
            vec![assertions::evaluate(
                &recipe,
                &assertion.expect("assertion case has an assertion"),
                &screen,
                &raw_output,
                exit_code,
            )]
        };
        let _ = tx.send(results);
    });
    // A thread that cannot be spawned is an environment failure, not a finding.
    spawned.expect("spawn case thread");

    match rx.recv_timeout(CASE_BUDGET) {
        Ok(results) => Outcome::Returned(results),
        // The sender is dropped without sending only when the case panicked.
        Err(RecvTimeoutError::Disconnected) => Outcome::Panicked(
            LAST_PANIC
                .lock()
                .expect("panic slot")
                .take()
                .unwrap_or_else(|| "panicked".to_string()),
        ),
        Err(RecvTimeoutError::Timeout) => Outcome::Stuck,
    }
}

fn render(results: &[AssertionResult], root: &str) -> Vec<(String, bool, String)> {
    results
        .iter()
        .map(|r| (redact(&r.name, root), r.passed, redact(&r.detail, root)))
        .collect()
}

fn expected_results(expected: &JsonValue) -> Vec<(String, bool, String)> {
    let rows = match expected.get("results") {
        Some(list) => list.as_array().expect("results array").clone(),
        None => vec![expected.clone()],
    };
    rows.iter()
        .map(|row| {
            (
                row["name"].as_str().unwrap_or_default().to_string(),
                row["passed"].as_bool().unwrap_or(false),
                row["detail"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

fn corpus_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/termproof.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../harness/corpus/assertions.expected.json")
}

#[test]
fn differential_assertions_against_python() {
    std::panic::set_hook(Box::new(|info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panicked".to_string());
        *LAST_PANIC.lock().expect("panic slot") = Some(message);
    }));

    let raw = std::fs::read_to_string(corpus_path()).expect("read recorded corpus");
    let document: JsonValue = serde_json::from_str(&raw).expect("parse recorded corpus");
    let cases = document["cases"].as_array().expect("corpus cases array");

    let fixture_dir = tempfile::Builder::new()
        .prefix("termproof-assert-")
        .tempdir()
        .expect("fixture root");
    let root = fixture_dir.path().to_string_lossy().into_owned();
    build_fixtures(fixture_dir.path(), &document["fixtures"]);

    let mut agreed = 0usize;
    let mut verdict_only = 0usize;
    let mut contained = 0usize;
    let mut escaped = 0usize;
    let mut panicked = 0usize;
    let mut stuck = 0usize;
    let mut divergences: Vec<String> = Vec::new();

    for case in cases {
        let id = case["id"].as_str().unwrap_or("<unnamed>");
        let expected = &case["expected"];
        let aborts = expected.get("aborts").and_then(JsonValue::as_bool) == Some(true);

        let results = match run_case(case, &root) {
            Outcome::Returned(results) => results,
            Outcome::Panicked(message) => {
                panicked += 1;
                divergences.push(format!("{id}:\n    rust:   PANIC {message:?}"));
                continue;
            }
            Outcome::Stuck => {
                stuck += 1;
                divergences.push(format!(
                    "{id}:\n    rust:   did not return within {CASE_BUDGET:?}"
                ));
                continue;
            }
        };
        let got = render(&results, &root);

        if aborts {
            // No oracle verdict to compare: the Python run ends here, losing
            // every result it had already collected. FR-020 requires the port
            // to keep going, so what is checked is that the offending assertion
            // came back as a reportable failure and nothing else was truncated.
            let well_formed = got.iter().all(|(name, _, detail)| {
                !name.is_empty() && !detail.is_empty() && !detail.contains('\n')
            });
            let reported = got.iter().any(|(_, passed, _)| !passed);
            if well_formed && reported {
                contained += 1;
            } else {
                escaped += 1;
                divergences.push(format!(
                    "{id}:\n    python: aborts ({} {})\n    rust:   {got:?} — not contained as a failure",
                    expected["exception"], expected["message"]
                ));
            }
            continue;
        }

        let want = expected_results(expected);
        let scores_agree = match expected.get("score") {
            Some(score) => {
                assertions::score(&results) == score.as_f64().expect("score number")
                    && results.iter().all(|r| r.passed)
                        == expected["passed"].as_bool().expect("overall verdict")
            }
            None => true,
        };

        if got == want && scores_agree {
            agreed += 1;
            continue;
        }
        let verdicts_agree = got.len() == want.len()
            && got.iter().zip(&want).all(|(g, w)| g.1 == w.1)
            && scores_agree;
        if verdicts_agree {
            verdict_only += 1;
        }
        divergences.push(format!("{id}:\n    python: {want:?}\n    rust:   {got:?}"));
    }

    let _ = std::panic::take_hook();

    let total = cases.len();
    let comparable = total - CONTAINED_EXACT;
    println!("\n=== differential harness: assertions ===");
    println!("oracle environment: {}", document["environment"]);
    for line in &divergences {
        println!("  {line}");
    }
    println!("\nfull agreement (name + passed + detail): {agreed}/{comparable}");
    println!("verdict agrees, detail differs:          {verdict_only}/{comparable}");
    println!(
        "passed/failed verdict agreement:         {}/{comparable}",
        agreed + verdict_only
    );
    println!("contained (oracle ends its run):         {contained}/{CONTAINED_EXACT}");
    println!("escaped containment:                     {escaped}");
    println!("panics:                                  {panicked}");
    println!("did not return:                          {stuck}");

    // Recipe-controlled input must never take the process down, wedge the run,
    // or truncate the report. None of these is a divergence to be traded off
    // against agreement — see FR-020.
    assert_eq!(panicked, 0, "{panicked} cases panicked");
    assert_eq!(stuck, 0, "{stuck} cases never returned");
    assert_eq!(escaped, 0, "{escaped} malformed inputs were not contained");
    assert_eq!(
        contained, CONTAINED_EXACT,
        "expected exactly {CONTAINED_EXACT} contained cases, found {contained}"
    );
    assert!(
        agreed >= AGREEMENT_FLOOR,
        "agreement dropped to {agreed}/{comparable}, below the recorded floor of {AGREEMENT_FLOOR}"
    );
    assert!(
        agreed + verdict_only >= VERDICT_FLOOR,
        "verdict agreement dropped to {}/{comparable}, below the recorded floor of {VERDICT_FLOOR}",
        agreed + verdict_only
    );
}
