//! `termproof run` executes recipes.
//!
//! It used to print a summary of its own arguments and exit 0 without
//! launching anything, so a recipe that could not possibly pass reported
//! success. These tests drive the built binary against real recipes and
//! assert on the evidence it leaves behind, because a claim in a PR body is
//! not a measurement.

use std::path::{Path, PathBuf};
use std::process::Command;

fn termproof() -> Command {
    Command::new(env!("CARGO_BIN_EXE_termproof"))
}

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termproof-cli-run-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write recipe");
    path
}

/// Every `result.json` written under `out`.
fn results(out: &Path) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    let entries = std::fs::read_dir(out).expect("out dir exists");
    for entry in entries.flatten() {
        let candidate = entry.path().join("result.json");
        if candidate.is_file() {
            let text = std::fs::read_to_string(&candidate).expect("read result.json");
            found.push(serde_json::from_str(&text).expect("result.json is json"));
        }
    }
    found
}

#[test]
fn a_recipe_is_actually_executed_and_its_evidence_written() {
    let dir = tempdir("executed");
    let recipe = write(
        &dir,
        "greet.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "greet",
          "command": {"argv": ["sh", "-c", "echo hello-from-the-cli"]},
          "steps": [
            {"name": "see the greeting", "action": "wait_for_text",
             "text": "hello-from-the-cli", "timeout_seconds": 5}
          ],
          "timeout_seconds": 10
        }"#,
    );
    let out = dir.join("out");

    let output = termproof()
        .args(["run"])
        .arg(&recipe)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run the recipe");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let results = results(&out);
    assert_eq!(
        results.len(),
        1,
        "expected one run directory; stdout: {stdout}"
    );
    let result = &results[0];
    assert_eq!(result["recipe_name"], "greet");
    assert_eq!(result["exit_code"], 0);

    let steps = result["steps"].as_array().expect("steps array");
    assert_eq!(steps.len(), 1, "the step must have run: {result}");
    assert_eq!(steps[0]["name"], "see the greeting");
    assert_eq!(
        steps[0]["passed"], true,
        "the step ran against a real child and should pass: {}",
        steps[0]["detail"]
    );
    assert!(
        steps[0]["screen"]
            .as_str()
            .unwrap_or_default()
            .contains("hello-from-the-cli"),
        "the screen captured at the step boundary should hold the output: {}",
        steps[0]["screen"]
    );

    assert!(
        stdout.contains("1 passed")
            || stdout.contains("0/1 passed")
            || stdout.contains("/1 passed"),
        "run should print a summary; got {stdout:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recipe_whose_step_cannot_pass_reports_failure() {
    let dir = tempdir("failing");
    let recipe = write(
        &dir,
        "impossible.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "impossible",
          "command": {"argv": ["sh", "-c", "echo something-else"]},
          "steps": [
            {"action": "wait_for_text", "text": "never-appears", "timeout_seconds": 1}
          ],
          "timeout_seconds": 5
        }"#,
    );
    let out = dir.join("out");
    let output = termproof()
        .args(["run"])
        .arg(&recipe)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run the recipe");

    assert_eq!(
        output.status.code(),
        Some(1),
        "a recipe that cannot pass must not exit 0; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let results = results(&out);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["passed"], false);
    assert_eq!(results[0]["steps"][0]["passed"], false);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_directory_of_recipes_is_discovered_and_each_one_runs() {
    let dir = tempdir("directory");
    let recipes = dir.join("recipes");
    std::fs::create_dir_all(&recipes).expect("recipes dir");
    for (name, marker) in [("alpha", "aaa"), ("beta", "bbb")] {
        write(
            &recipes,
            &format!("{name}.recipe.json"),
            &format!(
                r#"{{
                  "recipe_version": 1,
                  "name": "{name}",
                  "command": {{"argv": ["sh", "-c", "echo {marker}"]}},
                  "steps": [{{"action": "wait_for_text", "text": "{marker}", "timeout_seconds": 5}}],
                  "timeout_seconds": 10
                }}"#
            ),
        );
    }
    // Not a recipe: must be ignored rather than parsed.
    write(&recipes, "notes.md", "# not a recipe\n");

    let out = dir.join("out");
    termproof()
        .args(["run"])
        .arg(&recipes)
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run the directory");

    let mut names: Vec<String> = results(&out)
        .iter()
        .map(|r| r["recipe_name"].as_str().unwrap_or_default().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_recipe_path_is_an_error_not_a_silent_success() {
    let dir = tempdir("missing");
    let output = termproof()
        .args(["run", "no-such-recipe.json", "--out"])
        .arg(dir.join("out"))
        .output()
        .expect("run a missing recipe");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no-such-recipe.json"),
        "the diagnostic should name the path"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_reports_are_written_alongside_the_results() {
    let dir = tempdir("reports");
    let recipe = write(
        &dir,
        "r.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "reported",
          "command": {"argv": ["sh", "-c", "echo reported"]},
          "steps": [{"action": "wait_for_text", "text": "reported", "timeout_seconds": 5}],
          "timeout_seconds": 10
        }"#,
    );
    let out = dir.join("out");
    let xml = dir.join("junit.xml");
    termproof()
        .args(["run"])
        .arg(&recipe)
        .arg("--out")
        .arg(&out)
        .arg("--xml-path")
        .arg(&xml)
        .output()
        .expect("run with reports");

    let latest = std::fs::read_to_string(out.join("latest-report.md")).expect("latest-report.md");
    assert!(latest.contains("reported"), "report: {latest}");
    let junit = std::fs::read_to_string(&xml).expect("junit xml");
    assert!(junit.contains("reported"), "junit: {junit}");

    let run_dir = std::fs::read_dir(&out)
        .expect("out")
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("result.json").is_file())
        .expect("a run directory");
    assert!(run_dir.join("report.md").is_file());
    assert!(
        run_dir.join("session.cast").is_file(),
        "the cast is recorded"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_non_pty_recipe_says_so_rather_than_reporting_a_verdict() {
    let dir = tempdir("nonpty");
    let recipe = write(
        &dir,
        "pipe.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "pipe",
          "command": {"argv": ["sh", "-c", "echo hi"], "pty": false}
        }"#,
    );
    let output = termproof()
        .args(["run"])
        .arg(&recipe)
        .arg("--out")
        .arg(dir.join("out"))
        .output()
        .expect("run a non-pty recipe");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pty"), "stderr was {stderr:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_recipe_name_filter_selects_a_subset() {
    let dir = tempdir("filter");
    let recipes = dir.join("recipes");
    std::fs::create_dir_all(&recipes).expect("recipes dir");
    for name in ["kept", "dropped"] {
        write(
            &recipes,
            &format!("{name}.recipe.json"),
            &format!(
                r#"{{"recipe_version": 1, "name": "{name}",
                     "command": {{"argv": ["sh", "-c", "echo {name}"]}},
                     "timeout_seconds": 5}}"#
            ),
        );
    }
    let out = dir.join("out");
    termproof()
        .args(["run"])
        .arg(&recipes)
        .arg("--recipe-name")
        .arg("kept")
        .arg("--out")
        .arg(&out)
        .output()
        .expect("run with a name filter");

    let names: Vec<String> = results(&out)
        .iter()
        .map(|r| r["recipe_name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(names, vec!["kept".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn usage_errors_are_still_reported_before_any_recipe_is_loaded() {
    let output = termproof()
        .args(["run", "dummy.json", "--parallel", "0"])
        .output()
        .expect("run parallel 0");
    assert_eq!(output.status.code(), Some(2));
}
