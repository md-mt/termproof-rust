//! Eight built-in assertions with Python parity (RUST-008, #101).
//!
//! Mirrors `termproof/builtin_assertions.py` exactly:
//! * `output_contains` / `output_not_contains` — substring check on `raw_output`.
//! * `screen_contains` / `screen_not_contains` — substring check on `screen`.
//! * `exit_code` — equality check on `Option<i32>`.
//! * `file_exists` — `cwd`-relative probe when `value` is relative.
//! * `file_contains` — reads `path` then substring-checks `value` in file text.
//! * `json_schema` — validates `raw_output.trim()` as JSON against an
//!   inline or `cwd`-relative schema (Draft 2020-12 via `jsonschema`).
//!
//! Diagnostics, ordering, and `name`/`detail` serialization match Python
//! helper-for-helper so the conformance corpus compares bytes after
//! normalization.

use std::path::Path;

use serde_json::Value as JsonValue;

use crate::result::AssertionResult;

// ---------------------------------------------------------------------------
// Helpers — mirror Python `builtin_assertions.py` helpers
// ---------------------------------------------------------------------------

/// Mirror `_contains` — substring check with custom detail passthrough.
///
/// Returns `(name, passed, detail)` so callers stay faithful to the Python
/// `AssertionResult(name, passed, detail)` construction.
fn contains(
    name: &str,
    haystack: &str,
    needle: &str,
    should_contain: bool,
    custom_detail: Option<&str>,
) -> AssertionResult {
    let found = haystack.contains(needle);
    let passed = if should_contain { found } else { !found };
    let detail = if let Some(d) = custom_detail {
        d.to_string()
    } else {
        // Python uses `!r` on needle in the default branch. We emulate
        // Python's `repr` with `format!("{:?}")` over needle and keep the
        // two verbs exactly: "contains" / "does not contain".
        let verb = if should_contain {
            "contains"
        } else {
            "does not contain"
        };
        format!("{verb} {needle:?}")
    };
    AssertionResult {
        name: name.to_string(),
        passed,
        detail,
    }
}

/// Mirror `_recipe_path` — cwd-relative path resolution.
///
/// * `value_path` is treated as `Path` verbatim. If absolute, returned as is.
/// * Otherwise joined with `recipe.command.cwd` if present, else `.`.
fn recipe_path(cwd: Option<&str>, value_path: &str) -> std::path::PathBuf {
    let candidate = Path::new(value_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    let base = Path::new(cwd.unwrap_or("."));
    base.join(candidate)
}

/// Mirror `_json_schema` — resolve `schema_path` / `schema` to a JSON schema.
///
/// Ordering matters: `schema_path` is checked first, matching Python's
/// `if schema_path is not None: ...` before the `dict` / `str` branches.
/// Returns `(schema_value, error)` where error is user-visible detail on
/// the read/parse path.
fn json_schema_from_assertion(
    cwd: Option<&str>,
    assertion: &JsonValue,
) -> Result<JsonValue, String> {
    // Branch 1: schema_path key present (even if null we treat as missing).
    if let Some(path_val) = assertion.get("schema_path") {
        if !path_val.is_null() {
            let path_str = path_val
                .as_str()
                .ok_or_else(|| "json_schema 'schema_path' must be a string".to_string())?;
            let path = recipe_path(cwd, path_str);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("schema file unreadable: {e}"))?;
            let parsed: JsonValue =
                serde_json::from_str(&text).map_err(|e| format!("invalid schema JSON: {}", e))?;
            return Ok(parsed);
        }
    }
    // Branch 2: schema key is a JSON object
    if let Some(schema_val) = assertion.get("schema") {
        if schema_val.is_object() {
            return Ok(schema_val.clone());
        }
        if let Some(s) = schema_val.as_str() {
            // String is treated as a file path relative to cwd.
            let path = recipe_path(cwd, s);
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("schema file unreadable: {e}"))?;
            let parsed: JsonValue =
                serde_json::from_str(&text).map_err(|e| format!("invalid schema JSON: {}", e))?;
            return Ok(parsed);
        }
        // schema present but neither object nor string and not null
        if !schema_val.is_null() {
            return Err("json_schema requires an object schema or schema path".to_string());
        }
    }
    Err("json_schema requires an object schema or schema path".to_string())
}

// ---- assertion evaluators (mirror class hierarchy) -------------------------

fn assertion_name(assertion: &JsonValue, fallback: &str) -> String {
    assertion
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

/// `output_contains` — substring check on `raw_output` (mirrors `OutputContains.evaluate`).
pub fn output_contains(
    assertion: &JsonValue,
    screen: &str,
    raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "output_contains");
    let needle = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return AssertionResult {
                name,
                passed: false,
                detail: "output_contains 'value' must be a string".to_string(),
            };
        }
    };
    let _ = screen;
    contains(
        &name,
        raw_output,
        needle,
        true,
        assertion.get("detail").and_then(|v| v.as_str()),
    )
}

/// `output_not_contains` — negated substring check on `raw_output`.
pub fn output_not_contains(
    assertion: &JsonValue,
    screen: &str,
    raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "output_not_contains");
    let needle = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return AssertionResult {
                name,
                passed: false,
                detail: "output_not_contains 'value' must be a string".to_string(),
            };
        }
    };
    let _ = screen;
    contains(
        &name,
        raw_output,
        needle,
        false,
        assertion.get("detail").and_then(|v| v.as_str()),
    )
}

/// `screen_contains` — substring check on `screen`.
pub fn screen_contains(
    assertion: &JsonValue,
    screen: &str,
    _raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "screen_contains");
    let needle = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return AssertionResult {
                name,
                passed: false,
                detail: "screen_contains 'value' must be a string".to_string(),
            };
        }
    };
    contains(
        &name,
        screen,
        needle,
        true,
        assertion.get("detail").and_then(|v| v.as_str()),
    )
}

/// `screen_not_contains` — negated substring check on `screen`.
pub fn screen_not_contains(
    assertion: &JsonValue,
    screen: &str,
    _raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "screen_not_contains");
    let needle = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return AssertionResult {
                name,
                passed: false,
                detail: "screen_not_contains 'value' must be a string".to_string(),
            };
        }
    };
    contains(
        &name,
        screen,
        needle,
        false,
        assertion.get("detail").and_then(|v| v.as_str()),
    )
}

/// `exit_code` — equality check on `Option<i32>` (mirrors `ExitCode.evaluate`).
pub fn exit_code(
    assertion: &JsonValue,
    _screen: &str,
    _raw_output: &str,
    exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "exit_code");
    // `value` must be numeric; fall back to string form the same way Python
    // shows it (Python's detail is `f\"expected {value}, got {exit_code}\"` with
    // plain formatting — so we preserve that branch).
    let value = assertion.get("value");
    let expected: Option<i64> = match value {
        Some(JsonValue::Number(n)) => n.as_i64(),
        Some(JsonValue::Bool(_)) => None, // bool must not count as exit code
        _ => None,
    };
    let Some(expected_i) = expected else {
        return AssertionResult {
            name,
            passed: false,
            detail: format!("exit_code 'value' must be an integer, got {:?}", value),
        };
    };
    // Python compares ints; in Rust we coerce Option<i32> to Option<i64>.
    let actual: Option<i64> = exit_code.map(|v| v as i64);
    let passed = actual == Some(expected_i);
    AssertionResult {
        name,
        passed,
        detail: format!(
            "expected {}, got {}",
            expected_i,
            match actual {
                Some(v) => v.to_string(),
                None => "None".to_string(),
            }
        ),
    }
}

/// `file_exists` — cwd-relative existence probe (mirrors `FileExists.evaluate`).
pub fn file_exists(
    cwd: Option<&str>,
    assertion: &JsonValue,
    _screen: &str,
    _raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "file_exists");
    let value = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            // Python never validates; `str(None)` would produce strange paths.
            // We mirror: if the raw JSON is not a string, emit a diagnostic
            // rather than panicking.
            let got = assertion.get("value").map(type_name).unwrap_or("None");
            return AssertionResult {
                name,
                passed: false,
                detail: format!("file_exists 'value' must be a string, got {got}"),
            };
        }
    };
    let path = recipe_path(cwd, value);
    AssertionResult {
        name,
        passed: path.exists(),
        // Python detail is `str(path)` — so display form, not debug.
        detail: path.display().to_string(),
    }
}

/// `file_contains` — reads `path` then checks `value` substring (mirrors `FileContains.evaluate`).
pub fn file_contains(
    cwd: Option<&str>,
    assertion: &JsonValue,
    _screen: &str,
    _raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "file_contains");
    let path_str = match assertion.get("path").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            let got = assertion.get("path").map(type_name).unwrap_or("None");
            return AssertionResult {
                name,
                passed: false,
                detail: format!("file_contains 'path' must be a string, got {got}"),
            };
        }
    };
    let needle = match assertion.get("value").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            let got = assertion.get("value").map(type_name).unwrap_or("None");
            return AssertionResult {
                name,
                passed: false,
                detail: format!("file_contains 'value' must be a string, got {got}"),
            };
        }
    };
    let path = recipe_path(cwd, path_str);
    // Python: `path.read_text(...) if path.exists() else ""`
    // So missing files produce haystack "" and a fail unless needle is "".
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    contains(&name, &text, needle, true, None)
}

/// `json_schema` — validates `raw_output.trim()` as JSON against an inline or
/// cwd-relative schema (mirrors `JsonSchema.evaluate`, Draft 2020-12).
pub fn json_schema(
    cwd: Option<&str>,
    assertion: &JsonValue,
    _screen: &str,
    raw_output: &str,
    _exit_code: Option<i32>,
) -> AssertionResult {
    let name = assertion_name(assertion, "json_schema");

    let schema = match json_schema_from_assertion(cwd, assertion) {
        Ok(s) => s,
        Err(msg) => {
            return AssertionResult {
                name,
                passed: false,
                detail: msg,
            };
        }
    };

    // Instance is raw_output.trim() — exact same as Python
    let instance: JsonValue = match serde_json::from_str(raw_output.trim()) {
        Ok(v) => v,
        Err(e) => {
            // Python exposes `exc.msg` (``Expecting value`` without location).
            // `serde_json` always appends `` at line X column Y``; strip that
            // suffix so the diagnostic matches the Python oracle after
            // normalization (corpus compares ``invalid JSON output: …``).
            let mut msg = e.to_string();
            if let Some(pos) = msg.find(" at line ") {
                msg.truncate(pos);
            }
            return AssertionResult {
                name,
                passed: false,
                detail: format!("invalid JSON output: {msg}"),
            };
        }
    };

    // Validate via jsonschema Draft 2020-12.
    // Use the compiled validator; mirror Python's error taxonomy:
    // - SchemaError (bad schema) -> "invalid schema: {message}"
    // - ValidationError (bad instance) -> "schema validation failed[.path]: {message}"
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => {
            return AssertionResult {
                name,
                passed: false,
                detail: format!("invalid schema: {e}"),
            };
        }
    };
    let mut errors: Vec<_> = validator.iter_errors(&instance).collect();
    if errors.is_empty() {
        return AssertionResult {
            name,
            passed: true,
            detail: "matches JSON schema".to_string(),
        };
    }
    errors.sort_by(|a, b| {
        a.instance_path
            .to_string()
            .cmp(&b.instance_path.to_string())
    });
    let first = &errors[0];
    let path = first.instance_path.to_string();
    let location = if path.is_empty() || path == "/" {
        String::new()
    } else {
        let dot = path
            .trim_start_matches('/')
            .split('/')
            .map(|seg| seg.replace("~1", "/").replace("~0", "~"))
            .collect::<Vec<_>>()
            .join(".");
        format!(" at {dot}")
    };
    AssertionResult {
        name,
        passed: false,
        detail: format!("schema validation failed{location}: {first}"),
    }
}

/// JSON Schema evaluation using `iter_errors` (the stable 0.33 API). Kept
/// as the primary path so error `instance_path` is accessible.
pub fn json_schema_iter(
    cwd: Option<&str>,
    assertion: &JsonValue,
    raw_output: &str,
) -> AssertionResult {
    // Alias for `json_schema` so callers can pick either name.
    json_schema(cwd, assertion, "", raw_output, None)
}

/// Public entrypoint: dispatch by `type` with snapshotted buffers.
///
/// * `recipe` supplies `command.cwd` for path resolution.
/// * `screen` / `raw_output` / `exit_code` are the **immutable run snapshot**
///   the spec demands (RUST-008 §8: assertion engine evaluates immutable
///   snapshots, not live session streams).
/// * Ordering is preserved — caller iterates `recipe.assertions` in order and
///   includes the trailing implicit `exit_code` last if `expect_exit_code` is set.
/// * Earlier failures do not suppress later assertions — every assertion is
///   evaluated regardless of a predecessor's pass/fail.
pub fn evaluate(
    recipe: &crate::models::Recipe,
    assertion: &JsonValue,
    screen: &str,
    raw_output: &str,
    exit_code: Option<i32>,
) -> AssertionResult {
    let kind = assertion
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let cwd = recipe.command.cwd.as_deref();
    match kind {
        "output_contains" => output_contains(assertion, screen, raw_output, exit_code),
        "output_not_contains" => output_not_contains(assertion, screen, raw_output, exit_code),
        "screen_contains" => screen_contains(assertion, screen, raw_output, exit_code),
        "screen_not_contains" => screen_not_contains(assertion, screen, raw_output, exit_code),
        "exit_code" => self::exit_code(assertion, screen, raw_output, exit_code),
        "file_exists" => file_exists(cwd, assertion, screen, raw_output, exit_code),
        "file_contains" => file_contains(cwd, assertion, screen, raw_output, exit_code),
        "json_schema" => json_schema_iter(cwd, assertion, raw_output),
        _ => AssertionResult {
            name: assertion_name(assertion, kind),
            passed: false,
            detail: format!("unknown assertion type: {kind}"),
        },
    }
}

/// Evaluate all assertions including the trailing implicit `expect_exit_code`
/// in the same order as `VerificationRunner.evaluate_assertions`.
///
/// The ordering contract: explicit assertions first (in file order), then
/// the synthetic `exit_code` if and only if `recipe.expect_exit_code.is_some()`.
pub fn evaluate_all(
    recipe: &crate::models::Recipe,
    screen: &str,
    raw_output: &str,
    exit_code: Option<i32>,
) -> Vec<AssertionResult> {
    let mut out = Vec::with_capacity(recipe.assertions.len() + 1);
    for a in &recipe.assertions {
        out.push(evaluate(recipe, a, screen, raw_output, exit_code));
    }
    if let Some(expected) = recipe.expect_exit_code {
        let synth = serde_json::json!({"type": "exit_code", "value": expected});
        out.push(self::exit_code(&synth, screen, raw_output, exit_code));
    }
    out
}

fn type_name(v: &JsonValue) -> &'static str {
    match v {
        JsonValue::Null => "NoneType",
        JsonValue::Bool(_) => "bool",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "str",
        JsonValue::Array(_) => "list",
        JsonValue::Object(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn recipe(cwd: Option<&str>) -> crate::models::Recipe {
        crate::models::Recipe {
            name: "t".into(),
            command: crate::models::CommandSpec {
                argv: vec!["tool".into()],
                cwd: cwd.map(|s| s.to_string()),
                env: Default::default(),
                pty: true,
            },
            ..Default::default()
        }
    }

    #[test]
    fn output_contains_pass_and_fail_and_detail() {
        let r = recipe(None);
        let a = json!({"type": "output_contains", "value": "hello"});
        assert!(evaluate(&r, &a, "", "hello world", None).passed);
        assert!(!evaluate(&r, &a, "", "something else", None).passed);
        let with_detail = json!({"type":"output_contains","value":"hello","detail":"custom"});
        assert_eq!(
            evaluate(&r, &with_detail, "", "other", None).detail,
            "custom"
        );
    }

    #[test]
    fn output_not_contains() {
        let r = recipe(None);
        let a = json!({"type":"output_not_contains","value":"secret"});
        assert!(evaluate(&r, &a, "", "hello", None).passed);
        assert!(!evaluate(&r, &a, "", "secret here", None).passed);
    }

    #[test]
    fn screen_contains_and_not() {
        let r = recipe(None);
        assert!(
            evaluate(
                &r,
                &json!({"type":"screen_contains","value":"hi"}),
                "hi!",
                "",
                None
            )
            .passed
        );
        assert!(
            !evaluate(
                &r,
                &json!({"type":"screen_contains","value":"hi"}),
                "bye",
                "",
                None
            )
            .passed
        );
        assert!(
            evaluate(
                &r,
                &json!({"type":"screen_not_contains","value":"hi"}),
                "bye",
                "",
                None
            )
            .passed
        );
        assert!(
            !evaluate(
                &r,
                &json!({"type":"screen_not_contains","value":"hi"}),
                "hi!",
                "",
                None
            )
            .passed
        );
    }

    #[test]
    fn exit_code_expected_got_format() {
        let r = recipe(None);
        let a = json!({"type":"exit_code","value":0});
        let ok = evaluate(&r, &a, "", "", Some(0));
        assert!(ok.passed);
        assert_eq!(ok.detail, "expected 0, got 0");
        let bad = evaluate(&r, &a, "", "", Some(1));
        assert!(!bad.passed);
        assert_eq!(bad.detail, "expected 0, got 1");
        let none = evaluate(&r, &a, "", "", None);
        assert!(!none.passed);
        assert_eq!(none.detail, "expected 0, got None");
    }

    #[test]
    fn file_exists_and_file_contains() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap().to_string();
        std::fs::write(dir.path().join("exists.txt"), "hello").unwrap();
        let r = recipe(Some(&cwd));
        assert!(
            evaluate(
                &r,
                &json!({"type":"file_exists","value":"exists.txt"}),
                "",
                "",
                None
            )
            .passed
        );
        assert!(
            !evaluate(
                &r,
                &json!({"type":"file_exists","value":"missing.txt"}),
                "",
                "",
                None
            )
            .passed
        );
        assert!(
            evaluate(
                &r,
                &json!({"type":"file_contains","path":"exists.txt","value":"hello"}),
                "",
                "",
                None
            )
            .passed
        );
        assert!(
            !evaluate(
                &r,
                &json!({"type":"file_contains","path":"exists.txt","value":"bye"}),
                "",
                "",
                None
            )
            .passed
        );
        // missing file -> haystack "" -> fail unless needle is ""
        assert!(
            !evaluate(
                &r,
                &json!({"type":"file_contains","path":"missing.txt","value":"x"}),
                "",
                "",
                None
            )
            .passed
        );
        // absolute path also works regardless of cwd
        let abs = dir.path().join("exists.txt").display().to_string();
        assert!(
            evaluate(
                &r,
                &json!({"type":"file_exists","value": abs}),
                "",
                "",
                None
            )
            .passed
        );
    }

    #[test]
    fn json_schema_inline_and_file_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap().to_string();
        // inline schema pass
        {
            let r = recipe(None);
            let a = json!({"type":"json_schema","schema":{"type":"object","required":["status"],"properties":{"status":{"const":"ok"}}}});
            assert!(evaluate(&r, &a, "", "{\"status\":\"ok\"}", None).passed);
            let fail = evaluate(&r, &a, "", "{\"status\":\"bad\"}", None);
            assert!(!fail.passed);
        }
        // schema_path relative to cwd
        {
            let schema_path = dir.path().join("s.json");
            std::fs::write(&schema_path, json!({"type":"object","required":["count"],"properties":{"count":{"type":"integer"}}}).to_string()).unwrap();
            let r = recipe(Some(&cwd));
            let a = json!({"type":"json_schema","schema_path":"s.json"});
            assert!(evaluate(&r, &a, "", "{\"count\": 5}", None).passed);
        }
        // schema as file path string (legacy: `schema` is str -> file)
        {
            let schema_path = dir.path().join("s2.json");
            std::fs::write(&schema_path, json!({"type":"string"}).to_string()).unwrap();
            let r = recipe(Some(&cwd));
            let a = json!({"type":"json_schema","schema":"s2.json"});
            assert!(evaluate(&r, &a, "", "\"hi\"", None).passed);
        }
        // invalid JSON output
        {
            let r = recipe(None);
            let a = json!({"type":"json_schema","schema":{"type":"object"}});
            let res = evaluate(&r, &a, "", "not json", None);
            assert!(!res.passed);
            assert!(res.detail.starts_with("invalid JSON output:"));
        }
        // invalid schema file
        {
            let r = recipe(None);
            let a = json!({"type":"json_schema","schema_path":"/no/such/schema.json"});
            let res = evaluate(&r, &a, "", "{}", None);
            assert!(!res.passed);
            assert!(res.detail.contains("schema file unreadable"));
        }
    }

    #[test]
    fn assertion_name_override() {
        let r = recipe(None);
        let a = json!({"type":"output_contains","name":"my-assert","value":"x"});
        assert_eq!(evaluate(&r, &a, "", "x", None).name, "my-assert");
    }

    #[test]
    fn unknown_type_is_failure() {
        let r = recipe(None);
        let a = json!({"type":"nope"});
        let res = evaluate(&r, &a, "", "", None);
        assert!(!res.passed);
        assert!(res.detail.contains("unknown assertion type"));
    }

    #[test]
    fn evaluate_all_preserves_order_and_implicit_exit_code() {
        let mut r = recipe(None);
        r.assertions = vec![
            json!({"type":"output_contains","value":"a"}),
            json!({"type":"output_contains","value":"b"}),
        ];
        r.expect_exit_code = Some(0);
        let out = evaluate_all(&r, "", "a only", Some(0));
        assert_eq!(out.len(), 3);
        assert!(out[0].passed); // a present
        assert!(!out[1].passed); // b missing
        assert_eq!(out[2].name, "exit_code");
        assert!(out[2].passed); // exit 0 == 0
                                // verify earlier failure does not suppress later assertion
        assert!(!out[1].passed);
        assert!(out[2].passed);
    }

    #[test]
    fn evaluate_all_none_exit_skips_synthetic() {
        let mut r = recipe(None);
        r.assertions = vec![json!({"type":"output_contains","value":"hi"})];
        r.expect_exit_code = None;
        let out = evaluate_all(&r, "", "hi", None);
        assert_eq!(out.len(), 1);
    }
}
