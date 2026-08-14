//! Validation of recipe values before execution.
//!
//! Validates recipes against the Draft 2020-12 JSON Schema (via `jsonschema`)
//! and adds config-dependent checks (unknown step/assertion names) plus
//! legacy-compatibility carve-outs matching the Python `recipe_schema.py`
//! logic (null tolerance for optional fields, integral-float rejection, path
//! formatting).

use serde_json::Value;
use std::collections::HashSet;

use crate::config::VerifierConfig;
use crate::schema::generate_recipe_schema;

/// Severity of a validation issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    /// Hard error; recipe must not be executed.
    Error,
    /// Warning; e.g. missing `recipe_version` on legacy recipes.
    Warning,
}

/// A single validation issue with a path and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Dotted path to the failing field (e.g. `command.argv`, `steps[0].action`).
    pub path: String,
    /// Human-readable message.
    pub message: String,
    /// Severity.
    pub severity: Severity,
}

impl ValidationIssue {
    /// Create an error-severity issue.
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            severity: Severity::Error,
        }
    }
    /// Create a warning-severity issue.
    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
            severity: Severity::Warning,
        }
    }
}

/// Returns `true` if any issue has `Error` severity.
pub fn has_errors(issues: &[ValidationIssue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}

/// Validate a raw recipe value (JSON) before typed deserialization.
///
/// Performs:
/// - `recipe_version` warning if missing, error if not `1`.
/// - JSON Schema validation (Draft 2020-12) with legacy-tolerance suppression.
/// - Legacy integral-float rejection for `cols`, `rows`, `expect_exit_code`.
/// - Unknown plugin name checks from `config`.
pub fn validate_recipe_value(data: &Value, config: &VerifierConfig) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    validate_recipe_version(data, &mut issues);
    validate_via_schema(data, &mut issues);
    validate_legacy_int_fields(data, &mut issues);
    validate_plugin_names(data, config, &mut issues);
    issues
}

/// Validate a typed `Recipe` by round-tripping through its JSON value.
pub fn validate_recipe(
    recipe: &crate::recipe::Recipe,
    config: &VerifierConfig,
) -> Vec<ValidationIssue> {
    let value = serde_json::to_value(recipe).expect("recipe serializes");
    validate_recipe_value(&value, config)
}

fn validate_recipe_version(data: &Value, issues: &mut Vec<ValidationIssue>) {
    match data.get("recipe_version") {
        None => {
            issues.push(ValidationIssue::warning(
                "recipe_version",
                "missing recipe_version; treating recipe as legacy v0.x",
            ));
        }
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        Some(Value::Bool(_)) | Some(_) => {
            // Python checks: not int or bool or !=1. Bool is subtype of int
            // in Python, so `true` must be rejected. JSON true is Bool.
            if let Some(Value::Bool(_)) = data.get("recipe_version") {
                issues.push(ValidationIssue::error("recipe_version", "must be 1"));
            } else if !matches!(data.get("recipe_version"), Some(Value::Number(n)) if n.as_u64() == Some(1))
            {
                issues.push(ValidationIssue::error("recipe_version", "must be 1"));
            }
        }
    }
}

fn validate_via_schema(data: &Value, issues: &mut Vec<ValidationIssue>) {
    let schema = generate_recipe_schema();
    // Build a validator for Draft 2020-12.
    let validator = match jsonschema::validator_for(&schema) {
        Ok(v) => v,
        Err(e) => {
            issues.push(ValidationIssue::error("$", format!("invalid schema: {e}")));
            return;
        }
    };
    for error in validator.iter_errors(data) {
        let instance_path = error.instance_path.to_string();
        // Skip recipe_version: handled by validate_recipe_version.
        if instance_path == "/recipe_version" {
            continue;
        }
        // Suppress legacy-tolerated nulls and integral floats owned elsewhere.
        if is_legacy_tolerated(data, &error) {
            continue;
        }
        if is_legacy_int_owned(&error) {
            continue;
        }
        let path = format_instance_path(&error);
        issues.push(ValidationIssue::error(path, error.to_string()));
    }
}

fn format_instance_path(error: &jsonschema::ValidationError<'_>) -> String {
    let path = error.instance_path.to_string();
    if path.is_empty() {
        // Root or missing-required: try to recover property name from message.
        let msg = error.to_string();
        if let Some(prop) = extract_required_property(&msg) {
            return prop;
        }
        return "$".to_string();
    }
    // Convert JSON Pointer `/command/argv` → `command.argv`, `/steps/0/action` → `steps[0].action`
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let mut out = String::new();
    for (i, part) in parts.iter().enumerate() {
        // Decode ~1, ~0 per JSON Pointer.
        let decoded = part.replace("~1", "/").replace("~0", "~");
        let is_index = decoded.chars().all(|c| c.is_ascii_digit()) && !decoded.is_empty();
        if is_index {
            out.push_str(&format!("[{decoded}]"));
        } else if i == 0 {
            out.push_str(&decoded);
        } else {
            // Previous char might be `]` already; then we want `.` before next string.
            out.push('.');
            out.push_str(&decoded);
        }
    }
    if out.is_empty() {
        "$".to_string()
    } else {
        out
    }
}

fn extract_required_property(message: &str) -> Option<String> {
    // jsonschema message like: "Additional properties are not allowed ('foo' was unexpected)" or
    // For required: look for `'prop' is a required property` (Python's jsonschema) but Rust's message differs.
    // Rust jsonschema for required: "Property `name` is required" or similar? Check.
    // We'll handle both forms.
    if let Some(start) = message.find('\'') {
        if let Some(end) = message[start + 1..].find('\'') {
            let prop = &message[start + 1..start + 1 + end];
            if message.contains("required") {
                return Some(prop.to_string());
            }
        }
    }
    // Alternative: backtick form: "Property `name` is required"
    if let Some(start) = message.find('`') {
        if let Some(end) = message[start + 1..].find('`') {
            let prop = &message[start + 1..start + 1 + end];
            if message.contains("required") {
                return Some(prop.to_string());
            }
        }
    }
    None
}

/// Legacy tolerance: inputs the Python legacy validator would have accepted
/// (null for optional fields, unchecked description/intent names) must not
/// become new errors.
fn is_legacy_tolerated(data: &Value, error: &jsonschema::ValidationError<'_>) -> bool {
    let path_str = error.instance_path.to_string();
    if path_str.is_empty() {
        return false;
    }
    // Top-level description/intent: legacy never type-checked them.
    if path_str == "/description" || path_str == "/intent" {
        return true;
    }
    // steps[i].name / assertions[i].name: legacy only checked action/type.
    if (path_str.starts_with("/steps/") || path_str.starts_with("/assertions/"))
        && path_str.ends_with("/name")
    {
        // Count slashes: /steps/0/name => 3 parts. Ensure it's exactly steps[i]/name.
        let parts: Vec<&str> = path_str.split('/').collect();
        if parts.len() == 4 {
            return true;
        }
    }
    // Null tolerance: legacy accepted null for these optional fields.
    let null_tolerant_top: HashSet<&str> = [
        "priority",
        "execution",
        "determinism",
        "timeout_seconds",
        "cols",
        "rows",
        "checks",
        "ci_paths",
        "operator",
        "renderers",
        "description",
        "intent",
    ]
    .into_iter()
    .collect();
    let null_tolerant_command: HashSet<&str> = ["cwd", "pty"].into_iter().collect();
    let null_tolerant_step: HashSet<&str> = ["timeout_seconds"].into_iter().collect();

    // Need to locate value at the error path; if it's not null, not tolerated.
    let value_at = pointer_get(data, &path_str);
    if value_at != Some(&Value::Null) {
        return false;
    }
    // Determine tolerance by path.
    if path_str.matches('/').count() == 1 {
        // Top-level: /priority etc.
        let key = path_str.trim_start_matches('/');
        return null_tolerant_top.contains(key);
    }
    if path_str.starts_with("/command/") && path_str.matches('/').count() == 2 {
        let key = path_str.trim_start_matches("/command/");
        return null_tolerant_command.contains(key);
    }
    if path_str.starts_with("/steps/") {
        let parts: Vec<&str> = path_str.split('/').collect();
        if parts.len() == 4
            && parts[1].chars().all(|c| c.is_ascii_digit())
            && parts[3] == "timeout_seconds"
        {
            return null_tolerant_step.contains("timeout_seconds");
        }
    }
    false
}

fn pointer_get<'a>(data: &'a Value, pointer: &str) -> Option<&'a Value> {
    data.pointer(pointer)
}

/// Suppress schema errors for integer fields owned by explicit legacy checks.
/// JSON Schema draft 2020-12 treats integral floats as `integer`, but the
/// frozen Python validator required actual `int` (`cols: 1.0` must error).
fn is_legacy_int_owned(error: &jsonschema::ValidationError<'_>) -> bool {
    matches!(
        error.instance_path.to_string().as_str(),
        "/cols" | "/rows" | "/expect_exit_code"
    )
}

fn validate_legacy_int_fields(data: &Value, issues: &mut Vec<ValidationIssue>) {
    for key in ["cols", "rows"] {
        if let Some(v) = data.get(key) {
            if !is_valid_positive_int(v) {
                // Only report if value is not null (null is tolerated elsewhere, but we still need int check for non-null floats).
                // For int fields, null is tolerated (so we skip if null).
                if *v != Value::Null {
                    issues.push(ValidationIssue::error(key, "must be a positive integer"));
                }
            }
        }
    }
    if let Some(v) = data.get("expect_exit_code") {
        if *v != Value::Null && !is_valid_int(v) {
            issues.push(ValidationIssue::error(
                "expect_exit_code",
                "must be an integer or null",
            ));
        }
    }
}

fn is_valid_positive_int(v: &Value) -> bool {
    match v {
        Value::Number(n) => {
            if let Some(i) = n.as_u64() {
                i > 0
            } else if let Some(i) = n.as_i64() {
                i > 0
            } else if let Some(f) = n.as_f64() {
                // If it's a float, it's invalid even if integral (e.g. 1.0).
                // Python's `isinstance(1.0, int)` is False, so must error.
                // JSON numbers that are integers but encoded with decimal point are floats.
                // We treat any f64 as invalid for this check; only u64/i64 are valid.
                let _ = f;
                false
            } else {
                false
            }
        }
        Value::Null => true, // handled elsewhere as tolerated
        _ => false,
    }
}

fn is_valid_int(v: &Value) -> bool {
    match v {
        Value::Number(n) => n.is_u64() || n.is_i64(),
        _ => false,
    }
}

fn validate_plugin_names(data: &Value, config: &VerifierConfig, issues: &mut Vec<ValidationIssue>) {
    if let Some(Value::Array(steps)) = data.get("steps") {
        for (i, step) in steps.iter().enumerate() {
            if let Some(Value::Object(map)) = step.as_object().map(|_| step) {
                if let Some(Value::String(action)) = map.get("action") {
                    if !config.steps.contains_key(action) {
                        issues.push(ValidationIssue::error(
                            format!("steps[{i}].action"),
                            format!("unknown step action {action:?}"),
                        ));
                    }
                }
            }
        }
    }
    if let Some(Value::Array(assertions)) = data.get("assertions") {
        for (i, assertion) in assertions.iter().enumerate() {
            if let Some(Value::Object(map)) = assertion.as_object().map(|_| assertion) {
                if let Some(Value::String(kind)) = map.get("type") {
                    if !config.assertions.contains_key(kind) {
                        issues.push(ValidationIssue::error(
                            format!("assertions[{i}].type"),
                            format!("unknown assertion type {kind:?}"),
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cfg() -> VerifierConfig {
        VerifierConfig::builtin()
    }

    #[test]
    fn valid_recipe_has_no_errors() {
        let data = json!({
            "recipe_version": 1,
            "name": "valid",
            "command": {"argv": ["echo", "hi"]},
        });
        let issues = validate_recipe_value(&data, &cfg());
        assert!(!has_errors(&issues));
    }

    #[test]
    fn missing_version_is_warning_only() {
        let data = json!({
            "name": "x",
            "command": {"argv": ["true"]},
        });
        let issues = validate_recipe_value(&data, &cfg());
        assert!(!has_errors(&issues));
        assert!(issues.iter().any(|i| i.severity == Severity::Warning));
    }

    #[test]
    fn unknown_step_is_error() {
        let data = json!({
            "recipe_version": 1,
            "name": "x",
            "command": {"argv": ["true"]},
            "steps": [{"action": "nope"}]
        });
        let issues = validate_recipe_value(&data, &cfg());
        assert!(has_errors(&issues));
    }

    #[test]
    fn integral_float_cols_is_error() {
        let data = json!({
            "recipe_version": 1,
            "name": "x",
            "command": {"argv": ["true"]},
            "cols": 1.0
        });
        let issues = validate_recipe_value(&data, &cfg());
        assert!(has_errors(&issues));
        assert!(issues.iter().any(|i| i.path == "cols"));
    }

    #[test]
    fn null_description_is_tolerated() {
        let data = json!({
            "recipe_version": 1,
            "name": "x",
            "command": {"argv": ["true"]},
            "description": null
        });
        let issues = validate_recipe_value(&data, &cfg());
        assert!(!has_errors(&issues));
    }
}
