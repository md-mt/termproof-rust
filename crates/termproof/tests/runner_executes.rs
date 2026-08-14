//! The runner drives a recipe against a real child and returns a run result.
//!
//! Before this existed, `ExecutionMode::execute` had no concrete
//! `ExecutionContext` to run against outside of contract doubles, so nothing
//! in the workspace could take a recipe and produce a `RunResult`.

use std::path::{Path, PathBuf};

use termproof::runner::{LoadedRecipe, Runner};

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("write recipe");
    path
}

fn tempdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("termproof-runner-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

#[test]
fn a_recipe_runs_its_steps_against_a_real_child() {
    let dir = tempdir("steps");
    let path = write(
        &dir,
        "echo.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "echo",
          "command": {"argv": ["sh", "-c", "echo marker-one; echo marker-two"]},
          "steps": [
            {"action": "wait_for_text", "text": "marker-one", "timeout_seconds": 5},
            {"action": "wait_for_text", "text": "marker-two", "timeout_seconds": 5}
          ],
          "timeout_seconds": 10
        }"#,
    );

    let loaded = LoadedRecipe::from_file(&path).expect("load");
    assert_eq!(loaded.recipe.name, "echo");
    assert_eq!(loaded.renderers, vec![("default".to_string(), Vec::new())]);

    let run_dir = dir.join("run");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &run_dir)
        .expect("run");

    assert_eq!(result.recipe_name, "echo");
    assert_eq!(result.renderer, "default");
    assert_eq!(result.steps.len(), 2);
    for step in &result.steps {
        assert!(step.passed, "{}: {}", step.name, step.detail);
    }
    assert!(
        result.steps[0].screen.contains("marker-one"),
        "each step carries the screen captured when it finished; got {:?}",
        result.steps[0].screen
    );
    assert_eq!(result.exit_code, Some(0));
    assert!(result.duration_seconds >= 0.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_step_stops_the_run_and_is_reported() {
    let dir = tempdir("failing");
    let path = write(
        &dir,
        "missing.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "missing",
          "command": {"argv": ["sh", "-c", "echo present"]},
          "steps": [
            {"action": "wait_for_text", "text": "never-printed", "timeout_seconds": 1},
            {"action": "wait_for_text", "text": "present", "timeout_seconds": 1}
          ]
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");

    assert_eq!(
        result.steps.len(),
        1,
        "execution stops at the first failed step"
    );
    assert!(!result.steps[0].passed);
    assert!(!result.passed);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn send_line_reaches_the_child() {
    let dir = tempdir("sendline");
    let path = write(
        &dir,
        "cat.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "cat",
          "command": {"argv": ["cat"]},
          "steps": [
            {"action": "send_line", "text": "typed-by-the-runner"},
            {"action": "wait_for_text", "text": "typed-by-the-runner", "timeout_seconds": 5}
          ],
          "expect_exit_code": null,
          "timeout_seconds": 5
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");
    for step in &result.steps {
        assert!(step.passed, "{}: {}", step.name, step.detail);
    }
    let raw_path = result
        .artifacts
        .get("raw_output")
        .expect("the raw output is kept as an artifact");
    let raw = std::fs::read_to_string(raw_path).expect("read raw output");
    assert!(
        raw.contains("typed-by-the-runner"),
        "raw output was {raw:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_non_pty_recipe_is_refused_rather_than_silently_given_a_pty() {
    let dir = tempdir("nonpty");
    let path = write(
        &dir,
        "pipe.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "pipe",
          "command": {"argv": ["sh", "-c", "echo hi"], "pty": false}
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let err = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect_err("non-pty execution is not wired");
    let msg = err.to_string();
    assert!(msg.contains("pty"), "unhelpful diagnostic: {msg}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_agent_driven_recipe_is_refused_rather_than_fabricated() {
    let dir = tempdir("agent");
    let path = write(
        &dir,
        "agent.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "agent",
          "execution": "agent-driven",
          "command": {"argv": ["sh", "-c", "echo hi"]}
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let err = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect_err("agent-driven execution is not wired");
    assert!(
        err.to_string().contains("agent-driven"),
        "unhelpful diagnostic: {err}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_recipe_with_no_steps_still_produces_a_result() {
    let dir = tempdir("nosteps");
    let path = write(
        &dir,
        "bare.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "bare",
          "command": {"argv": ["sh", "-c", "exit 7"]},
          "expect_exit_code": 7,
          "timeout_seconds": 5
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");
    assert!(result.steps.is_empty());
    assert_eq!(result.exit_code, Some(7));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_yaml_recipe_loads_the_same_as_json() {
    let dir = tempdir("yaml");
    let path = write(
        &dir,
        "y.recipe.yaml",
        "recipe_version: 1\nname: yaml-recipe\ncommand:\n  argv: [sh, -c, \"echo yamlish\"]\nsteps:\n  - action: wait_for_text\n    text: yamlish\n    timeout_seconds: 5\n",
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load yaml");
    assert_eq!(loaded.recipe.name, "yaml-recipe");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");
    assert!(result.steps[0].passed, "{}", result.steps[0].detail);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_renderer_table_is_read_from_the_recipe() {
    let dir = tempdir("renderers");
    let path = write(
        &dir,
        "r.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "r",
          "command": {"argv": ["true"]},
          "renderers": {"wide": ["--cols", "200"], "default": []}
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    assert_eq!(
        loaded.renderers,
        vec![
            ("default".to_string(), Vec::new()),
            (
                "wide".to_string(),
                vec!["--cols".to_string(), "200".to_string()]
            ),
        ],
        "renderers must be sorted so plans are reproducible"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The end-to-end proof that the shared assertions are wired to a real run:
/// `Runner` supplies no `evaluate_assertion` of its own, so what runs here is
/// the trait's default body — `assertions::evaluate`.
#[test]
fn assertions_are_evaluated_against_what_the_target_actually_did() {
    let dir = tempdir("assertions");
    let path = write(
        &dir,
        "a.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "a",
          "command": {"argv": ["sh", "-c", "echo hi"]},
          "assertions": [{"type": "output_contains", "value": "hi"}],
          "timeout_seconds": 5
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");

    // The declared assertion plus the implicit exit_code one (003-FR-019).
    assert_eq!(result.assertions.len(), 2);
    assert_eq!(result.assertions[0].name, "output_contains");
    assert_eq!(result.assertions[0].detail, "contains 'hi'");
    assert_eq!(result.assertions[1].name, "exit_code");
    assert_eq!(result.assertions[1].detail, "expected 0, got 0");
    for assertion in &result.assertions {
        assert!(
            assertion.passed,
            "the target printed hi and exited 0: {:?}",
            assertion.detail
        );
    }
    assert!(result.passed, "every step and every assertion passed");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The other half: an assertion the target does not satisfy fails the run, and
/// says what it was looking for rather than that nobody looked.
#[test]
fn a_failing_assertion_fails_the_run() {
    let dir = tempdir("assertions-fail");
    let path = write(
        &dir,
        "a.recipe.json",
        r#"{
          "recipe_version": 1,
          "name": "a",
          "command": {"argv": ["sh", "-c", "echo hi"]},
          "assertions": [{"type": "output_contains", "value": "definitely-absent"}],
          "timeout_seconds": 5
        }"#,
    );
    let loaded = LoadedRecipe::from_file(&path).expect("load");
    let result = Runner::new()
        .run(&loaded.recipe, "default", &dir.join("run"))
        .expect("run");

    assert!(!result.assertions[0].passed);
    assert_eq!(result.assertions[0].detail, "contains 'definitely-absent'");
    assert!(result.assertions[1].passed, "the exit code was still 0");
    assert!(!result.passed);
    let _ = std::fs::remove_dir_all(&dir);
}
