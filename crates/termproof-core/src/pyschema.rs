//! JSON Schema validation for the `json_schema` assertion.
//!
//! Two things live here, both of which spec 003 leaves the port to build rather
//! than borrow:
//!
//! 1. **Which error to report.** FR-017 requires exactly one, chosen the way
//!    Python's `jsonschema.best_match` chooses it. That is a ranking algorithm,
//!    not a string, so it is reimplemented here from its documented behaviour:
//!    prefer the shallowest error; among siblings prefer the later path; demote
//!    `anyOf`/`oneOf`; demote an error whose enclosing subschema declares a
//!    type the instance already satisfies; then descend into the winner's
//!    context unless its two best children rank equally.
//!
//! 2. **What that error says.** The oracle interpolates `jsonschema`'s own
//!    message. Embedding another project's message table is the open decision
//!    003-OQ-010 (= 001-OQ-001 = 002-OQ-002), and it is not the port's to make,
//!    so this reaches the same verdict and says so in TermProof's words. The
//!    FR-016 prefixes — `schema validation failed`, `invalid schema:` — are
//!    unreserved contract and are preserved exactly; only the interpolated
//!    clause is ours. `harness/README.md` records the divergence.
//!
//! Every message here renders the instance through [`crate::pyrepr`], never
//! Rust's `Debug` (constitution Principle VIII, restated as FR-025).

use jsonschema::error::{TypeKind, ValidationErrorKind};
use jsonschema::{ValidationError, Validator};
use serde_json::Value as JsonValue;

use crate::pyrepr::repr_json;

/// Compile a schema, or describe why it is not a schema.
///
/// The `Err` is the clause that goes after FR-016's `invalid schema: ` prefix.
pub fn compile(schema: &JsonValue) -> Result<Validator, String> {
    jsonschema::validator_for(schema).map_err(|error| describe(&error))
}

/// Validate `instance`, returning the clause for FR-016's
/// `schema validation failed` detail plus the dotted instance path FR-017 asks
/// for, or `None` when the instance is valid.
pub fn validate(validator: &Validator, instance: &JsonValue) -> Option<(String, String)> {
    let errors: Vec<ValidationError<'_>> = validator.iter_errors(instance).collect();
    let best = best_match(&errors)?;
    Some((dotted_path(best.instance_path.as_str()), describe(best)))
}

/// A JSON Pointer as FR-017 wants it rendered: components joined with `.`, and
/// an array index as a bare integer, so an element of a list reads `at 11`
/// rather than `at [11]`.
fn dotted_path(pointer: &str) -> String {
    pointer
        .split('/')
        .skip(1)
        .map(|part| part.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>()
        .join(".")
}

/// How the port ranks one error, highest wins. The tuple mirrors
/// `jsonschema.exceptions.by_relevance`, whose ordering FR-017 pins down by
/// worked example rather than by prose.
///
/// The oracle's tuple has a fourth component the port cannot compute: whether
/// the instance satisfies a `type` declared alongside the failing keyword. It
/// needs the enclosing subschema, which the Rust crate does not hand back with
/// the error. It only ever separates two non-`type` errors sitting under
/// differently-typed subschemas; everywhere else it is constant and drops out.
/// Approximating it as constant is recorded in `harness/README.md` rather than
/// hidden.
type Relevance = (isize, Vec<PathPart>, bool);

/// A path component ordered the way Python orders the parts of an
/// `error.path`. A path never mixes the two at one position, because the
/// instance at that level is either an object or an array.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum PathPart {
    Index(u64),
    Property(String),
}

fn relevance(error: &ValidationError<'_>) -> Relevance {
    let parts: Vec<PathPart> = error
        .instance_path
        .as_str()
        .split('/')
        .skip(1)
        .map(|part| match part.parse::<u64>() {
            Ok(index) => PathPart::Index(index),
            Err(_) => PathPart::Property(part.replace("~1", "/").replace("~0", "~")),
        })
        .collect();
    (
        // Shallower is more relevant: it means more of the instance is wrong.
        -(parts.len() as isize),
        parts,
        !is_weak(&error.kind),
    )
}

/// `anyOf` and `oneOf` only need one branch to succeed, so a failure of either
/// says less than a sibling failure does.
fn is_weak(kind: &ValidationErrorKind) -> bool {
    matches!(
        kind,
        ValidationErrorKind::AnyOf { .. }
            | ValidationErrorKind::OneOfNotValid { .. }
            | ValidationErrorKind::OneOfMultipleValid { .. }
    )
}

/// `max(errors, key=relevance)` then the context descent, transcribed.
///
/// `max` keeps the first of equally-ranked candidates, so iteration order is
/// the final tie-break — the same way it is in the oracle, and just as
/// dependent on the library's keyword order.
fn best_match<'e>(errors: &'e [ValidationError<'e>]) -> Option<&'e ValidationError<'e>> {
    let mut best = errors.first()?;
    let mut best_key = relevance(best);
    for error in &errors[1..] {
        let key = relevance(error);
        if key > best_key {
            best = error;
            best_key = key;
        }
    }
    descend(best)
}

/// Follow the winner into its `anyOf`/`oneOf` context, stopping where the two
/// best children rank equally — at that point the branches disagree about
/// nothing in particular and the parent is the more honest answer.
fn descend<'e>(mut best: &'e ValidationError<'e>) -> Option<&'e ValidationError<'e>> {
    loop {
        let context = match &best.kind {
            ValidationErrorKind::AnyOf { context }
            | ValidationErrorKind::OneOfNotValid { context }
            | ValidationErrorKind::OneOfMultipleValid { context } => context,
            _ => return Some(best),
        };
        let flat: Vec<&ValidationError<'static>> = context.iter().flatten().collect();
        let mut sorted: Vec<&ValidationError<'static>> = flat.clone();
        if sorted.is_empty() {
            return Some(best);
        }
        sorted.sort_by_key(|error| relevance(error));
        if sorted.len() >= 2 && relevance(sorted[0]) == relevance(sorted[1]) {
            return Some(best);
        }
        best = sorted[0];
    }
}

/// TermProof's own wording for a validation failure.
///
/// Written against `ValidationErrorKind`'s structured fields rather than the
/// library's `Display`, so a library upgrade cannot silently reword a report.
fn describe(error: &ValidationError<'_>) -> String {
    let instance = repr_json(&error.instance);
    match &error.kind {
        ValidationErrorKind::Type { kind } => {
            format!("{instance} is not of type {}", type_names(kind))
        }
        ValidationErrorKind::Required { property } => {
            format!("{} is a required property", repr_json(property))
        }
        ValidationErrorKind::AdditionalProperties { unexpected } => {
            format!("unexpected properties {}", quoted_list(unexpected))
        }
        ValidationErrorKind::UnevaluatedProperties { unexpected } => {
            format!("unevaluated properties {}", quoted_list(unexpected))
        }
        ValidationErrorKind::Enum { options } => {
            format!("{instance} is not one of {}", repr_json(options))
        }
        ValidationErrorKind::Constant { expected_value } => {
            format!("{instance} does not equal {}", repr_json(expected_value))
        }
        ValidationErrorKind::Pattern { pattern } => {
            format!("{instance} does not match pattern {}", quoted(pattern))
        }
        ValidationErrorKind::Format { format } => {
            format!("{instance} is not a valid {}", quoted(format))
        }
        ValidationErrorKind::Minimum { limit } => {
            format!("{instance} is less than the minimum {}", repr_json(limit))
        }
        ValidationErrorKind::Maximum { limit } => {
            format!(
                "{instance} is greater than the maximum {}",
                repr_json(limit)
            )
        }
        ValidationErrorKind::ExclusiveMinimum { limit } => format!(
            "{instance} is not greater than the exclusive minimum {}",
            repr_json(limit)
        ),
        ValidationErrorKind::ExclusiveMaximum { limit } => format!(
            "{instance} is not less than the exclusive maximum {}",
            repr_json(limit)
        ),
        ValidationErrorKind::MultipleOf { multiple_of } => format!(
            "{instance} is not a multiple of {}",
            crate::pyrepr::repr_f64(*multiple_of)
        ),
        ValidationErrorKind::MinLength { limit } => {
            format!("{instance} is shorter than {limit} characters")
        }
        ValidationErrorKind::MaxLength { limit } => {
            format!("{instance} is longer than {limit} characters")
        }
        ValidationErrorKind::MinItems { limit } => {
            format!("{instance} has fewer than {limit} items")
        }
        ValidationErrorKind::MaxItems { limit } => {
            format!("{instance} has more than {limit} items")
        }
        ValidationErrorKind::AdditionalItems { limit } => {
            format!("{instance} has more than {limit} items")
        }
        ValidationErrorKind::UnevaluatedItems { unexpected } => {
            format!("unevaluated items {}", quoted_list(unexpected))
        }
        ValidationErrorKind::MinProperties { limit } => {
            format!("{instance} has fewer than {limit} properties")
        }
        ValidationErrorKind::MaxProperties { limit } => {
            format!("{instance} has more than {limit} properties")
        }
        ValidationErrorKind::UniqueItems => format!("{instance} has non-unique elements"),
        ValidationErrorKind::Contains => format!("{instance} contains no matching item"),
        ValidationErrorKind::AnyOf { .. } | ValidationErrorKind::OneOfNotValid { .. } => {
            format!("{instance} does not match any of the given schemas")
        }
        ValidationErrorKind::OneOfMultipleValid { .. } => {
            format!("{instance} matches more than one of the given schemas")
        }
        ValidationErrorKind::Not { .. } => {
            format!("{instance} is valid under the negated schema")
        }
        ValidationErrorKind::FalseSchema => format!("{instance} is not allowed here"),
        ValidationErrorKind::PropertyNames { error } => {
            format!("invalid property name: {}", describe(error))
        }
        ValidationErrorKind::Custom { message } => message.clone(),
        // The remaining kinds report a malformed schema, an unresolvable `$ref`
        // or a regex the engine gave up on. They all mean the same thing to a
        // recipe author, and none of them can name the instance usefully.
        _ => format!("{instance} does not satisfy the schema"),
    }
}

/// The `type` keyword's value, rendered the way the keyword spells it.
fn type_names(kind: &TypeKind) -> String {
    match kind {
        TypeKind::Single(one) => quoted(&one.to_string()),
        TypeKind::Multiple(set) => set
            .iter()
            .map(|one| quoted(&one.to_string()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn quoted(value: &str) -> String {
    repr_json(&JsonValue::String(value.to_string()))
}

fn quoted_list(values: &[String]) -> String {
    values
        .iter()
        .map(|v| quoted(v))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn detail(schema: JsonValue, instance: JsonValue) -> (String, String) {
        let validator = compile(&schema).expect("schema compiles");
        validate(&validator, &instance).expect("instance is invalid")
    }

    #[test]
    fn a_valid_instance_produces_no_error() {
        let validator = compile(&json!({"type": "object"})).expect("schema compiles");
        assert_eq!(validate(&validator, &json!({"a": 1})), None);
    }

    #[test]
    fn the_instance_renders_with_python_repr() {
        let (path, message) = detail(json!({"type": "array"}), json!({"a": 1}));
        assert_eq!(path, "");
        assert_eq!(message, "{'a': 1} is not of type 'array'");
    }

    /// FR-017: an array index is a bare integer in the path, not `[11]`.
    #[test]
    fn an_array_index_is_a_bare_integer_in_the_path() {
        let (path, _) = detail(json!({"items": {"type": "string"}}), json!(["a", 2]));
        assert_eq!(path, "1");
    }

    #[test]
    fn nested_properties_join_with_dots() {
        let schema = json!({"properties": {"a": {"properties": {"b": {"type": "string"}}}}});
        let (path, message) = detail(schema, json!({"a": {"b": 1}}));
        assert_eq!(path, "a.b");
        assert_eq!(message, "1 is not of type 'string'");
    }

    /// FR-017 requires exactly one error, and the shallowest one: a root
    /// failure says more is wrong than a leaf failure does.
    #[test]
    fn the_shallowest_error_wins() {
        let schema = json!({
            "required": ["zzz"],
            "properties": {"a": {"type": "string"}},
        });
        let (path, message) = detail(schema, json!({"a": 1}));
        assert_eq!(path, "");
        assert_eq!(message, "'zzz' is a required property");
    }

    /// A `maxItems` failure at the root outranks three item failures below it.
    #[test]
    fn a_root_error_outranks_its_children() {
        let schema = json!({"maxItems": 1, "items": {"type": "string"}});
        let (path, message) = detail(schema, json!([1, 2, 3]));
        assert_eq!(path, "");
        assert_eq!(message, "[1, 2, 3] has more than 1 items");
    }

    /// `anyOf` whose branches fail equally stops at the parent rather than
    /// picking a branch arbitrarily.
    #[test]
    fn equally_bad_branches_report_the_parent() {
        let schema = json!({"anyOf": [{"type": "string"}, {"type": "number"}]});
        let (path, message) = detail(schema, json!({}));
        assert_eq!(path, "");
        assert_eq!(message, "{} does not match any of the given schemas");
    }

    #[test]
    fn a_schema_that_is_not_a_schema_is_described_not_panicked_on() {
        let error = compile(&json!({"type": "nope"})).expect_err("not a schema");
        assert!(!error.is_empty());
        assert!(!error.contains('\n'), "FR-021: details are single-line");
    }

    /// Constitution Principle VIII / FR-025: no `Debug` rendering reaches a
    /// detail, so a string in a message is Python-quoted and never Rust-quoted.
    #[test]
    fn messages_use_python_quoting_throughout() {
        let (_, message) = detail(json!({"const": "it's"}), json!("x"));
        assert_eq!(message, "'x' does not equal \"it's\"");
    }
}
