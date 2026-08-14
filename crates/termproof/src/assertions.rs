//! The eight built-in assertions (spec 003).
//!
//! One shared implementation, the way [`crate::steps`] is one shared
//! implementation. Before this module `ExecutionContext::evaluate_assertion`
//! was a required trait method with no in-tree body outside a test double, so
//! every execution mode was free to answer differently; now they share this and
//! `harness/corpus/assertion_cases.json` measures it.
//!
//! | Type | Reads | Detail |
//! |---|---|---|
//! | `output_contains` | `raw_output` | `contains <repr>` |
//! | `output_not_contains` | `raw_output` | `does not contain <repr>` |
//! | `screen_contains` | `screen` | `contains <repr>` |
//! | `screen_not_contains` | `screen` | `does not contain <repr>` |
//! | `exit_code` | `exit_code` | `expected <str>, got <str>` |
//! | `file_exists` | the filesystem | the resolved path |
//! | `file_contains` | the filesystem | `contains <repr>` |
//! | `json_schema` | `raw_output` | one of nine (FR-016) |
//!
//! `json_schema` is the one row behind a feature. It needs the `jsonschema`
//! crate, which is 87 of the crate's 180 transitive dependencies, so the
//! `json-schema` feature — on by default — decides whether it is compiled. Off,
//! it is absent from [`BUILTIN_TYPES`] and dispatch reports it the way it
//! reports any name it does not know; the other seven are unaffected.
//!
//! **Nothing here can end a run.** FR-020 supersedes the oracle, which raises
//! out of `evaluate_assertions` on eight different malformed inputs and
//! discards every result already collected. Each of those becomes a failed
//! `AssertionResult` instead, so a report is never truncated by an earlier
//! assertion — a partial report is worse than none.

use std::collections::BTreeMap;
#[cfg(feature = "json-schema")]
use std::fmt::Write as _;

use serde_json::Value as JsonValue;

use crate::models::{AssertionResult, Recipe};
use crate::pypath::PyPath;
use crate::pyrepr::{repr_json, repr_str, str_json, type_name};
#[cfg(feature = "json-schema")]
use crate::pyschema;

/// The built-ins that need no optional dependency, in the order the config
/// declares them.
///
/// The single source for both shapes of [`BUILTIN_TYPES`]. Writing the list
/// once is what makes "the same names minus `json_schema`" a fact the compiler
/// carries rather than a convention two literals have to keep: an eighth
/// unconditional built-in is added here, and both shapes gain it.
const UNCONDITIONAL_TYPES: [&str; 7] = [
    "output_contains",
    "output_not_contains",
    "screen_contains",
    "screen_not_contains",
    "exit_code",
    "file_exists",
    "file_contains",
];

/// The assertion types this crate answers for, in the order the config
/// declares them.
///
/// FR-024 wants dispatch to go through a registry rather than a closed `match`,
/// so a plugin type can be added without editing the dispatcher. This is that
/// registry's built-in half; [`dispatch`] consults it by name.
///
/// The list is what is actually dispatchable: [`UNCONDITIONAL_TYPES`], and
/// `json_schema` when the `json-schema` feature compiles something that can
/// answer it.
#[cfg(feature = "json-schema")]
pub const BUILTIN_TYPES: [&str; 8] = {
    let mut all = [""; 8];
    let mut i = 0;
    while i < UNCONDITIONAL_TYPES.len() {
        all[i] = UNCONDITIONAL_TYPES[i];
        i += 1;
    }
    all[i] = "json_schema";
    all
};

/// The assertion types this crate answers for, without the `json-schema`
/// feature: [`UNCONDITIONAL_TYPES`] exactly, since nothing else is compiled.
#[cfg(not(feature = "json-schema"))]
pub const BUILTIN_TYPES: [&str; 7] = UNCONDITIONAL_TYPES;

/// The assertions a recipe evaluates, in order (FR-019).
///
/// The recipe's own list, then a synthetic `exit_code` when `expect_exit_code`
/// is set. An explicit `exit_code` assertion does not suppress the synthetic
/// one — the recipe is evaluated twice against the same number, and both
/// results count toward the score. That is the oracle's behaviour and 003-OQ-002
/// asks whether it should stay.
pub fn evaluated_list(recipe: &Recipe) -> Vec<JsonValue> {
    let mut all = recipe.assertions.clone();
    if let Some(expected) = recipe.expect_exit_code {
        all.push(serde_json::json!({"type": "exit_code", "value": expected}));
    }
    all
}

/// Evaluate every assertion a recipe declares, including the synthetic one.
pub fn evaluate_all(
    recipe: &Recipe,
    screen: &str,
    raw_output: &str,
    exit_code: Option<i32>,
) -> Vec<AssertionResult> {
    evaluated_list(recipe)
        .iter()
        .map(|assertion| evaluate(recipe, assertion, screen, raw_output, exit_code))
        .collect()
}

/// Evaluate one assertion.
///
/// Never panics and never returns `Err`: an unrecognised type, a missing key or
/// a value of the wrong type all come back as a failed result (FR-020).
pub fn evaluate(
    recipe: &Recipe,
    assertion: &JsonValue,
    screen: &str,
    raw_output: &str,
    exit_code: Option<i32>,
) -> AssertionResult {
    let name = name_of(assertion);
    match dispatch(recipe, assertion, screen, raw_output, exit_code) {
        Ok(outcome) => AssertionResult {
            name,
            passed: outcome.passed,
            detail: outcome.detail,
        },
        Err(detail) => AssertionResult {
            name,
            passed: false,
            detail,
        },
    }
}

/// What an assertion decided, before the name is attached.
struct Outcome {
    passed: bool,
    detail: String,
}

impl Outcome {
    fn new(passed: bool, detail: String) -> Self {
        Self { passed, detail }
    }
}

/// The `name` key if present, else the type name (FR-002).
///
/// Unlike a step, an assertion's name carries no index prefix, so two
/// assertions of the same type are indistinguishable in a report unless the
/// recipe names them.
///
/// A malformed assertion still needs a name. The type is used when it is a
/// string, and `assertion` when there is nothing else to call it — the oracle
/// never reaches this point, because it has already ended the run.
fn name_of(assertion: &JsonValue) -> String {
    if let Some(name) = assertion.get("name").and_then(JsonValue::as_str) {
        return name.to_string();
    }
    match assertion.get("type").and_then(JsonValue::as_str) {
        Some(kind) => kind.to_string(),
        None => "assertion".to_string(),
    }
}

/// Route to the assertion named by `type`.
///
/// `Err` is the detail of a contained failure: either the input was not a
/// well-formed assertion, or it named a type nothing implements.
fn dispatch(
    recipe: &Recipe,
    assertion: &JsonValue,
    screen: &str,
    raw_output: &str,
    exit_code: Option<i32>,
) -> Result<Outcome, String> {
    let kind = match assertion.get("type") {
        Some(JsonValue::String(kind)) => kind.as_str(),
        Some(other) => return Err(format!("unknown assertion type {}", repr_json(other))),
        None => return Err("assertion has no 'type'".to_string()),
    };
    match kind {
        "output_contains" => contains(assertion, raw_output, true),
        "output_not_contains" => contains(assertion, raw_output, false),
        "screen_contains" => contains(assertion, screen, true),
        "screen_not_contains" => contains(assertion, screen, false),
        "exit_code" => exit_code_matches(assertion, exit_code),
        "file_exists" => file_exists(recipe, assertion),
        "file_contains" => file_contains(recipe, assertion),
        #[cfg(feature = "json-schema")]
        "json_schema" => json_schema(recipe, assertion, raw_output),
        other => Err(format!("unknown assertion type {}", repr_str(other))),
    }
}

/// Read a required key, or say which one is missing.
fn require<'a>(assertion: &'a JsonValue, key: &str, kind: &str) -> Result<&'a JsonValue, String> {
    assertion
        .get(key)
        .ok_or_else(|| format!("{kind} requires a {}", repr_str(key)))
}

/// Read a required string, or say what arrived instead.
///
/// The oracle raises `TypeError: 'in <string>' requires string as left operand`
/// here and loses the run. FR-020 says it must not, and 003-OQ-001 leaves the
/// wording open; this follows 002's convention of naming the key, the expected
/// type and the type that arrived.
fn require_str<'a>(assertion: &'a JsonValue, key: &str, kind: &str) -> Result<&'a str, String> {
    let value = require(assertion, key, kind)?;
    value.as_str().ok_or_else(|| {
        format!(
            "{kind} {} must be a string, got {}",
            repr_str(key),
            type_name(value)
        )
    })
}

/// The four contains-family assertions (FR-004).
///
/// The detail states the *expectation*, not the outcome, and states it
/// identically whether the assertion passed or failed — `contains 'zzz'` on a
/// failure reads like a claim rather than a complaint. That is the oracle's
/// wording and 003-OQ-004 asks whether it should stay.
///
/// An empty needle is in every string, so `output_contains` with `value: ""`
/// always passes and `output_not_contains` always fails.
fn contains(assertion: &JsonValue, haystack: &str, want: bool) -> Result<Outcome, String> {
    let kind = assertion["type"].as_str().unwrap_or("assertion");
    let needle = require_str(assertion, "value", kind)?;
    let found = haystack.contains(needle);
    let expectation = if want { "contains" } else { "does not contain" };
    Ok(Outcome::new(
        found == want,
        custom_detail(assertion).unwrap_or_else(|| format!("{expectation} {}", repr_str(needle))),
    ))
}

/// The `detail` override (FR-005).
///
/// An empty string does not override, because the oracle writes
/// `custom_detail or <generated>` and `""` is falsy in Python. Four of the
/// eight assertions honour the key at all; 003-OQ-005 covers both surprises.
fn custom_detail(assertion: &JsonValue) -> Option<String> {
    match assertion.get("detail").and_then(JsonValue::as_str) {
        Some("") | None => None,
        Some(detail) => Some(detail.to_string()),
    }
}

/// `exit_code` (FR-008, FR-009).
///
/// Comparison is Python's `==` across types: `True == 1`, `False == 0`,
/// `0.0 == 0`, and a string never equals an int. The detail interpolates with
/// `str`, so `value: "0"` against exit code `0` fails and still reads
/// `expected 0, got 0` — the detail cannot be used to tell the two apart, which
/// is 003-OQ-003.
fn exit_code_matches(assertion: &JsonValue, exit_code: Option<i32>) -> Result<Outcome, String> {
    let expected = require(assertion, "value", "exit_code")?;
    let actual = match exit_code {
        Some(code) => JsonValue::from(code),
        None => JsonValue::Null,
    };
    Ok(Outcome::new(
        python_eq(expected, exit_code),
        format!("expected {}, got {}", str_json(expected), str_json(&actual)),
    ))
}

/// Python's `==` between a recipe value and an exit code.
fn python_eq(expected: &JsonValue, exit_code: Option<i32>) -> bool {
    let Some(code) = exit_code else {
        // `None == None`; nothing else equals `None`.
        return expected.is_null();
    };
    match expected {
        // `bool` is a subclass of `int`, so `True == 1` and `False == 0`.
        JsonValue::Bool(b) => i64::from(*b) == i64::from(code),
        JsonValue::Number(n) => match n.as_i64() {
            Some(i) => i == i64::from(code),
            // A float equals an int when it is exactly that int.
            None => n.as_f64() == Some(f64::from(code)),
        },
        // A str, list, dict or None never equals an int.
        _ => false,
    }
}

/// Resolve a recipe-relative path (FR-010, FR-011).
///
/// An absolute path is used as-is; a relative one is joined to `command.cwd`,
/// or to `.` when the recipe does not set one. The join is lexical — pathlib
/// drops `./` and collapses duplicate separators but keeps `..`, and nothing
/// canonicalises or resolves a symlink. So `sub/../exists.txt` is *not*
/// `exists.txt`: the kernel resolves the `..` when the path is stat-ed, which
/// fails if `sub` does not exist. 003-OQ-006 asks whether that should stay.
fn recipe_path(recipe: &Recipe, path: &str) -> PyPath {
    let candidate = PyPath::parse(path);
    if candidate.is_absolute() {
        return candidate;
    }
    PyPath::parse(recipe.command.cwd.as_deref().unwrap_or(".")).join(&candidate)
}

/// `file_exists` (FR-012).
///
/// The value is stringified with `str`, so a non-string becomes a path: `5` is
/// the file `5`, and `value: ""` is the working directory itself. The detail is
/// the resolved path and nothing else — it does not say whether the file was
/// found, which the `passed` field already carries.
fn file_exists(recipe: &Recipe, assertion: &JsonValue) -> Result<Outcome, String> {
    let value = require(assertion, "value", "file_exists")?;
    let path = recipe_path(recipe, &str_json(value));
    let rendered = path.to_string();
    Ok(Outcome::new(
        std::path::Path::new(&rendered).exists(),
        rendered,
    ))
}

/// `file_contains` (FR-013, FR-014).
///
/// A missing file reads as the empty string rather than as an error, so
/// `file_contains` cannot distinguish "the file is not there" from "the file
/// does not say that" — 003-OQ-007. It does not honour the `detail` override,
/// unlike the four contains-family assertions it delegates to.
///
/// The oracle reads the file as strict UTF-8 and a byte that is not UTF-8 ends
/// the run. FR-020 does not allow that, so the read is lossy: the assertion
/// reaches a verdict on the decodable text instead of taking the report down.
fn file_contains(recipe: &Recipe, assertion: &JsonValue) -> Result<Outcome, String> {
    let path = require(assertion, "path", "file_contains")?;
    let needle = require_str(assertion, "value", "file_contains")?;
    let resolved = recipe_path(recipe, &str_json(path)).to_string();
    let text = std::fs::read(&resolved)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    Ok(Outcome::new(
        text.contains(needle),
        format!("contains {}", repr_str(needle)),
    ))
}

// Everything from here to `describe_json_error` serves `json_schema` and
// nothing else, so the `json-schema` feature gates it as one block.

/// `json_schema` (FR-015, FR-016, FR-017).
///
/// Six of the nine details the oracle produces embed a `jsonschema`, CPython or
/// libc message. Reproducing another project's message table is the open
/// decision 003-OQ-010, so the port reaches the same verdict and words the
/// clause itself. The prefixes are unreserved contract (FR-021) and are exact.
#[cfg(feature = "json-schema")]
fn json_schema(
    recipe: &Recipe,
    assertion: &JsonValue,
    raw_output: &str,
) -> Result<Outcome, String> {
    let schema = resolve_schema(recipe, assertion)?;
    let validator = match pyschema::compile(&schema) {
        Ok(validator) => validator,
        Err(why) => return Ok(Outcome::new(false, format!("invalid schema: {why}"))),
    };
    let instance: JsonValue = match serde_json::from_str(raw_output.trim()) {
        Ok(instance) => instance,
        Err(error) => {
            return Ok(Outcome::new(
                false,
                format!(
                    "invalid JSON output: {}",
                    describe_json_error(raw_output, &error)
                ),
            ))
        }
    };
    match pyschema::validate(&validator, &instance) {
        None => Ok(Outcome::new(true, "matches JSON schema".to_string())),
        Some((path, why)) => {
            let mut detail = String::from("schema validation failed");
            if !path.is_empty() {
                let _ = write!(detail, " at {path}");
            }
            let _ = write!(detail, ": {why}");
            Ok(Outcome::new(false, detail))
        }
    }
}

/// Where the schema comes from (FR-015), in the oracle's order.
///
/// `schema_path` first if it is present *and not null* — an explicit
/// `schema_path: null` falls through to `schema` rather than erroring. Then
/// `schema` as an object, then `schema` as a path. Anything else is the fixed
/// FR-016 message.
///
/// Both file branches return the schema-reading detail as `Ok(Err(..))` would
/// in a language with one; here a failure is an `Err(String)` that
/// [`evaluate`] turns into a failed result with that detail.
#[cfg(feature = "json-schema")]
fn resolve_schema(recipe: &Recipe, assertion: &JsonValue) -> Result<JsonValue, String> {
    match assertion.get("schema_path") {
        Some(JsonValue::Null) | None => {}
        Some(path) => return read_schema_file(recipe, &str_json(path)),
    }
    match assertion.get("schema") {
        Some(JsonValue::Object(map)) => Ok(JsonValue::Object(map.clone())),
        Some(JsonValue::String(path)) => read_schema_file(recipe, path),
        _ => Err("json_schema requires an object schema or schema path".to_string()),
    }
}

/// Read and parse a schema file, with FR-016's two prefixes.
#[cfg(feature = "json-schema")]
fn read_schema_file(recipe: &Recipe, path: &str) -> Result<JsonValue, String> {
    let resolved = recipe_path(recipe, path).to_string();
    let text = std::fs::read_to_string(&resolved).map_err(|error| {
        format!(
            "schema file unreadable: {}",
            describe_io_error(&error, &resolved)
        )
    })?;
    serde_json::from_str(&text).map_err(|error| {
        format!(
            "invalid schema JSON: {}",
            describe_json_error(&text, &error)
        )
    })
}

/// TermProof's own wording for a filesystem failure.
///
/// The oracle interpolates libc's, via `OSError.__str__`:
/// `[Errno 2] No such file or directory: '/tmp/fx/nope.json'`. That is
/// 003-OQ-010's territory, so this says the same thing in its own words and
/// keeps the path, which is the part a recipe author needs.
#[cfg(feature = "json-schema")]
fn describe_io_error(error: &std::io::Error, path: &str) -> String {
    use std::io::ErrorKind;
    let cause = match error.kind() {
        ErrorKind::NotFound => "no such file or directory",
        ErrorKind::PermissionDenied => "permission denied",
        ErrorKind::IsADirectory => "is a directory",
        ErrorKind::InvalidData => "not valid UTF-8",
        _ => "cannot be read",
    };
    format!("{cause}: {}", repr_str(path))
}

/// TermProof's own wording for a JSON parse failure.
///
/// The oracle interpolates CPython's `JSONDecodeError.msg` — `Expecting value`,
/// `Expecting property name enclosed in double quotes`,
/// `Unexpected UTF-8 BOM (decode using utf-8-sig)` — with the line and column
/// suffix stripped (FR-017). Same 003-OQ-010 decision, same treatment: the
/// verdict matches, the wording is the port's, and no detail carries a newline
/// (FR-021 / SC-009).
///
/// The oracle's decoder also *accepts* `NaN`, `Infinity`, `-Infinity` and
/// duplicate keys, which is 003-OQ-008 — a recipe asserting a program emits
/// valid JSON passes on output no other parser accepts. `serde_json` rejects
/// the non-finite tokens and cannot represent them, so those four corpus rows
/// diverge on `passed`, not merely on wording. Recorded, not papered over.
#[cfg(feature = "json-schema")]
fn describe_json_error(input: &str, error: &serde_json::Error) -> String {
    if input.starts_with('\u{feff}') {
        return "unexpected UTF-8 byte order mark".to_string();
    }
    if input.trim().is_empty() {
        return "no output to parse".to_string();
    }
    match error.classify() {
        serde_json::error::Category::Eof => "unexpected end of input".to_string(),
        serde_json::error::Category::Data => format!(
            "unexpected value at line {} column {}",
            error.line(),
            error.column()
        ),
        _ => format!(
            "invalid syntax at line {} column {}",
            error.line(),
            error.column()
        ),
    }
}

/// The score a set of assertion results earns (FR-022): the fraction that
/// passed, and `1.0` when there are none to fail.
pub fn score(results: &[AssertionResult]) -> f64 {
    if results.is_empty() {
        return 1.0;
    }
    let passed = results.iter().filter(|r| r.passed).count();
    passed as f64 / results.len() as f64
}

/// The built-in registry, for a caller that needs to know whether a name is
/// claimed before dispatching (FR-024).
pub fn builtin_registry() -> BTreeMap<&'static str, ()> {
    BUILTIN_TYPES.iter().map(|name| (*name, ())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CommandSpec, Recipe};
    use serde_json::json;

    fn recipe_with_cwd(cwd: Option<&str>) -> Recipe {
        Recipe {
            name: "t".into(),
            command: CommandSpec {
                argv: vec!["true".into()],
                cwd: cwd.map(str::to_string),
                ..CommandSpec::default()
            },
            ..Recipe::default()
        }
    }

    fn eval(assertion: JsonValue, screen: &str, raw: &str, code: Option<i32>) -> AssertionResult {
        evaluate(&recipe_with_cwd(None), &assertion, screen, raw, code)
    }

    #[test]
    fn contains_searches_only_its_own_haystack() {
        let a = eval(
            json!({"type": "output_contains", "value": "S"}),
            "S",
            "R",
            None,
        );
        assert!(!a.passed);
        let b = eval(
            json!({"type": "screen_contains", "value": "R"}),
            "S",
            "R",
            None,
        );
        assert!(!b.passed);
    }

    /// FR-004: the detail states the expectation identically on pass and fail.
    #[test]
    fn the_contains_detail_is_the_same_either_way() {
        let hit = eval(
            json!({"type": "output_contains", "value": "R"}),
            "",
            "R",
            None,
        );
        let miss = eval(
            json!({"type": "output_contains", "value": "R"}),
            "",
            "",
            None,
        );
        assert_eq!(hit.detail, "contains 'R'");
        assert_eq!(miss.detail, "contains 'R'");
        assert!(hit.passed && !miss.passed);
    }

    #[test]
    fn an_empty_needle_is_always_present() {
        assert!(
            eval(
                json!({"type": "output_contains", "value": ""}),
                "",
                "",
                None
            )
            .passed
        );
        assert!(
            !eval(
                json!({"type": "output_not_contains", "value": ""}),
                "",
                "",
                None
            )
            .passed
        );
    }

    /// FR-005: `detail` replaces the generated string, but `""` does not.
    #[test]
    fn an_empty_detail_override_falls_through() {
        let overridden = eval(
            json!({"type": "output_contains", "value": "R", "detail": "mine"}),
            "",
            "R",
            None,
        );
        assert_eq!(overridden.detail, "mine");
        let empty = eval(
            json!({"type": "output_contains", "value": "R", "detail": ""}),
            "",
            "R",
            None,
        );
        assert_eq!(empty.detail, "contains 'R'");
    }

    /// FR-002: `name` wins, else the type; no index prefix, unlike a step.
    #[test]
    fn the_name_defaults_to_the_type() {
        assert_eq!(
            eval(
                json!({"type": "screen_contains", "value": ""}),
                "",
                "",
                None
            )
            .name,
            "screen_contains"
        );
        assert_eq!(
            eval(
                json!({"type": "screen_contains", "value": "", "name": "mine"}),
                "",
                "",
                None
            )
            .name,
            "mine"
        );
    }

    /// FR-008, every row of the table.
    #[test]
    fn exit_code_compares_the_way_python_compares() {
        let rows: [(JsonValue, Option<i32>, bool, &str); 8] = [
            (json!(0), Some(0), true, "expected 0, got 0"),
            (json!(1), Some(0), false, "expected 1, got 0"),
            (json!(true), Some(1), true, "expected True, got 1"),
            (json!(false), Some(0), true, "expected False, got 0"),
            (json!(0.0), Some(0), true, "expected 0.0, got 0"),
            (json!("0"), Some(0), false, "expected 0, got 0"),
            (json!(0), None, false, "expected 0, got None"),
            (json!(null), None, true, "expected None, got None"),
        ];
        for (value, code, passed, detail) in rows {
            let result = eval(json!({"type": "exit_code", "value": value}), "", "", code);
            assert_eq!(result.passed, passed, "{detail}");
            assert_eq!(result.detail, detail);
        }
    }

    /// FR-011: lexical joining, `..` preserved, `""` naming the directory.
    #[test]
    fn paths_join_the_way_pathlib_joins_them() {
        let recipe = recipe_with_cwd(Some("/tmp/fx"));
        let cases = [
            ("/tmp/fx/exists.txt", "/tmp/fx/exists.txt"),
            ("exists.txt", "/tmp/fx/exists.txt"),
            ("./exists.txt", "/tmp/fx/exists.txt"),
            ("sub/../exists.txt", "/tmp/fx/sub/../exists.txt"),
            ("", "/tmp/fx"),
        ];
        for (input, want) in cases {
            let result = evaluate(
                &recipe,
                &json!({"type": "file_exists", "value": input}),
                "",
                "",
                None,
            );
            assert_eq!(result.detail, want, "{input}");
        }
        let trailing = recipe_with_cwd(Some("/tmp/fx//"));
        assert_eq!(
            evaluate(
                &trailing,
                &json!({"type": "file_exists", "value": "exists.txt"}),
                "",
                "",
                None
            )
            .detail,
            "/tmp/fx/exists.txt"
        );
    }

    /// FR-012: the value is stringified, so a number names a file.
    #[test]
    fn file_exists_stringifies_whatever_it_is_given() {
        let recipe = recipe_with_cwd(Some("/tmp/fx"));
        for (value, want) in [
            (json!(5), "/tmp/fx/5"),
            (json!(true), "/tmp/fx/True"),
            (json!(null), "/tmp/fx/None"),
        ] {
            let result = evaluate(
                &recipe,
                &json!({"type": "file_exists", "value": value}),
                "",
                "",
                None,
            );
            assert_eq!(result.detail, want);
            assert!(!result.passed);
        }
    }

    #[test]
    fn file_contains_reads_a_missing_file_as_empty() {
        let recipe = recipe_with_cwd(Some("/tmp"));
        let result = evaluate(
            &recipe,
            &json!({"type": "file_contains", "path": "definitely-absent-xyz", "value": "x"}),
            "",
            "",
            None,
        );
        assert!(!result.passed);
        assert_eq!(result.detail, "contains 'x'");
    }

    /// FR-014: unlike the four contains assertions, this one ignores `detail`.
    #[test]
    fn file_contains_ignores_the_detail_override() {
        let recipe = recipe_with_cwd(Some("/tmp"));
        let result = evaluate(
            &recipe,
            &json!({"type": "file_contains", "path": "absent", "value": "x", "detail": "mine"}),
            "",
            "",
            None,
        );
        assert_eq!(result.detail, "contains 'x'");
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn json_schema_reports_the_fixed_message_when_there_is_no_schema() {
        for assertion in [
            json!({"type": "json_schema"}),
            json!({"type": "json_schema", "schema": []}),
            json!({"type": "json_schema", "schema": 5}),
            json!({"type": "json_schema", "schema": null}),
        ] {
            let result = eval(assertion, "", "{}", None);
            assert!(!result.passed);
            assert_eq!(
                result.detail,
                "json_schema requires an object schema or schema path"
            );
        }
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn json_schema_passes_a_matching_instance() {
        let result = eval(
            json!({"type": "json_schema", "schema": {"type": "object"}}),
            "",
            "  {\"a\": 1}  ",
            None,
        );
        assert!(result.passed);
        assert_eq!(result.detail, "matches JSON schema");
    }

    /// FR-016: the prefix and the `at <path>` infix are contract.
    #[cfg(feature = "json-schema")]
    #[test]
    fn json_schema_names_where_the_instance_failed() {
        let result = eval(
            json!({"type": "json_schema", "schema": {"properties": {"a": {"type": "string"}}}}),
            "",
            "{\"a\": 1}",
            None,
        );
        assert_eq!(
            result.detail,
            "schema validation failed at a: 1 is not of type 'string'"
        );
    }

    /// FR-015: an explicit null `schema_path` falls through to `schema`.
    #[cfg(feature = "json-schema")]
    #[test]
    fn a_null_schema_path_is_not_a_path() {
        let result = eval(
            json!({"type": "json_schema", "schema_path": null, "schema": {"type": "object"}}),
            "",
            "{}",
            None,
        );
        assert!(result.passed);
    }

    #[cfg(feature = "json-schema")]
    #[test]
    fn a_missing_schema_file_says_so_with_its_path() {
        let recipe = recipe_with_cwd(Some("/tmp"));
        let result = evaluate(
            &recipe,
            &json!({"type": "json_schema", "schema_path": "definitely-absent-xyz.json"}),
            "",
            "{}",
            None,
        );
        assert_eq!(
            result.detail,
            "schema file unreadable: no such file or directory: '/tmp/definitely-absent-xyz.json'"
        );
    }

    /// FR-020, the requirement that supersedes the oracle: none of the eight
    /// inputs that end a Python run may end this one.
    #[test]
    fn every_malformed_input_is_contained() {
        let malformed = [
            json!({"value": "x"}),
            json!({"type": "no_such_type"}),
            json!({"type": null}),
            json!({"type": "output_contains"}),
            json!({"type": "exit_code"}),
            json!({"type": "file_contains", "value": "x"}),
            json!({"type": "output_contains", "value": 5}),
            json!({"type": "output_contains", "value": null}),
        ];
        for assertion in malformed {
            let result = eval(assertion.clone(), "S", "R", Some(0));
            assert!(!result.passed, "{assertion}");
            assert!(!result.detail.is_empty(), "{assertion}");
            assert!(!result.detail.contains('\n'), "{assertion}");
            assert!(!result.name.is_empty(), "{assertion}");
        }
    }

    /// A failing assertion must not truncate the list — the report is the
    /// evidence, and a partial one is worse than none.
    #[test]
    fn a_malformed_assertion_does_not_lose_the_others() {
        let mut recipe = recipe_with_cwd(None);
        recipe.assertions = vec![
            json!({"type": "output_contains", "value": "R"}),
            json!({"type": "output_contains"}),
            json!({"type": "screen_contains", "value": "S"}),
        ];
        recipe.expect_exit_code = Some(0);
        let results = evaluate_all(&recipe, "S", "R", Some(0));
        assert_eq!(results.len(), 4);
        assert_eq!(
            results.iter().map(|r| r.passed).collect::<Vec<_>>(),
            vec![true, false, true, true]
        );
    }

    /// FR-019: recipe order, then the synthetic exit_code, which an explicit
    /// one does not suppress.
    #[test]
    fn the_synthetic_exit_code_goes_last_and_is_never_suppressed() {
        let mut recipe = recipe_with_cwd(None);
        recipe.assertions = vec![json!({"type": "exit_code", "value": 3})];
        recipe.expect_exit_code = Some(0);
        let results = evaluate_all(&recipe, "", "", Some(0));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].detail, "expected 3, got 0");
        assert_eq!(results[1].detail, "expected 0, got 0");

        recipe.expect_exit_code = None;
        assert_eq!(evaluate_all(&recipe, "", "", Some(0)).len(), 1);
    }

    /// FR-022, all five shapes.
    #[test]
    fn the_score_is_the_passing_fraction() {
        let pass = AssertionResult {
            name: "a".into(),
            passed: true,
            detail: String::new(),
        };
        let fail = AssertionResult {
            passed: false,
            ..pass.clone()
        };
        assert_eq!(score(&[]), 1.0);
        assert_eq!(score(std::slice::from_ref(&pass)), 1.0);
        assert_eq!(score(std::slice::from_ref(&fail)), 0.0);
        assert_eq!(score(&[pass.clone(), fail.clone()]), 0.5);
        assert_eq!(score(&[pass, fail.clone(), fail]), 1.0 / 3.0);
    }

    /// The count itself is pinned by `BUILTIN_TYPES`'s array type, which the
    /// `json-schema` feature picks; what this checks is that every name in it
    /// reaches an implementation.
    #[test]
    fn the_registry_names_every_builtin() {
        assert_eq!(builtin_registry().len(), BUILTIN_TYPES.len());
        for name in BUILTIN_TYPES {
            let result = eval(json!({"type": name}), "", "", None);
            assert!(
                !result.detail.starts_with("unknown assertion type"),
                "{name} is not dispatched"
            );
        }
    }

    /// `BUILTIN_TYPES` is derived from `UNCONDITIONAL_TYPES`, so the compiler
    /// already carries "the same names minus `json_schema`". What it cannot
    /// carry is that the *dispatcher* agrees, since a match arm is not a value.
    /// `json_schema` is the one name whose presence varies, so this pins the
    /// registry, the dispatcher and the feature to each other at that point —
    /// in both directions, and in whichever shape is compiled.
    #[test]
    fn json_schema_is_dispatchable_exactly_when_the_registry_lists_it() {
        let listed = BUILTIN_TYPES.contains(&"json_schema");
        let detail = eval(json!({"type": "json_schema"}), "", "", None).detail;
        let dispatched = !detail.starts_with("unknown assertion type");

        assert_eq!(
            listed, dispatched,
            "registry says {listed}, dispatcher says {dispatched}"
        );
        assert_eq!(listed, cfg!(feature = "json-schema"));
        assert_eq!(
            BUILTIN_TYPES.len(),
            UNCONDITIONAL_TYPES.len() + usize::from(listed)
        );
        for name in UNCONDITIONAL_TYPES {
            assert!(BUILTIN_TYPES.contains(&name), "{name} was dropped");
        }
    }
}
