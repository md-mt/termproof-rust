//! Snapshot guard for [`termproof::schema::generate_recipe_schema`] (#33).
//!
//! The unit tests next to `generate_recipe_schema` assert only `$schema`, two
//! `required` entries and the `recipe_version` const, so the generated schema
//! can be rewritten structurally — `definitions` to `$defs` with every `$ref`
//! retargeted, `minimum: 0.0` to `minimum: 0`, key and `required` ordering —
//! while those tests stay green. That is exactly what a `schemars` major bump
//! did, and nothing noticed.
//!
//! This test pins the crate's own generated schema to a checked-in snapshot
//! (`tests/snapshots/recipe_schema_v1.json`). Both sides are parsed and
//! compared as `serde_json::Value` trees, not as text, so object key order in
//! the file is ignored (it is semantically irrelevant), but every structural
//! difference — keywords, numbers, array order, `$ref` targets — is caught.
//!
//! What this guard proves, and what it does not:
//!
//! - it **does** catch accidental changes to our generated schema;
//! - it does **not** establish agreement with the canonical schema, which
//!   lives outside this repository and is not vendored here. That remains
//!   parity-gate work.
//!
//! Re-blessing is deliberate and never the default. To update the snapshot
//! after an intentional schema change:
//!
//! ```text
//! TERM_PROOF_BLESS_SCHEMA=1 cargo test -p termproof --test schema_snapshot
//! ```
//!
//! The env var name follows the existing `TERM_PROOF_*` convention in this
//! crate. The snapshot file ships in the published package (`cargo package
//! --list` shows it), so a consumer testing the published crate gets the same
//! check.

#![cfg(feature = "schema")]

use std::path::PathBuf;

use termproof::schema::generate_recipe_schema;

/// Absolute path of the checked-in snapshot, anchored to the crate root so
/// the test works both in the workspace and inside a published tarball.
fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("snapshots")
        .join("recipe_schema_v1.json")
}

/// Returns a JSON-pointer-like path to the first difference between two
/// values, or `None` when they are equal.
///
/// Object keys are compared as a set (order-insensitive, matching JSON
/// semantics); arrays compare in order.
fn first_difference_path(a: &serde_json::Value, b: &serde_json::Value) -> Option<String> {
    if a == b {
        return None;
    }
    match (a, b) {
        (serde_json::Value::Object(am), serde_json::Value::Object(bm)) => {
            let mut keys: Vec<&str> = am
                .keys()
                .chain(bm.keys())
                .map(String::as_str)
                .collect::<Vec<_>>();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let same = match (am.get(key), bm.get(key)) {
                    (Some(av), Some(bv)) => av == bv,
                    _ => false,
                };
                if !same {
                    let suffix = match (am.get(key), bm.get(key)) {
                        (Some(av), Some(bv)) => first_difference_path(av, bv).unwrap_or_default(),
                        _ => String::new(),
                    };
                    return Some(format!("/{key}{suffix}"));
                }
            }
            Some(String::new())
        }
        (serde_json::Value::Array(aa), serde_json::Value::Array(ba)) => {
            for (index, (av, bv)) in aa.iter().zip(ba.iter()).enumerate() {
                if av != bv {
                    let suffix = first_difference_path(av, bv).unwrap_or_default();
                    return Some(format!("/{index}{suffix}"));
                }
            }
            Some(format!("/{}", aa.len().min(ba.len())))
        }
        _ => Some(String::new()),
    }
}

#[test]
fn generated_schema_matches_checked_in_snapshot() {
    let generated = generate_recipe_schema();
    let path = snapshot_path();

    // Explicit, non-default re-bless flow. Only this env var ever writes the
    // snapshot; without it the test is read-only.
    if std::env::var_os("TERM_PROOF_BLESS_SCHEMA").is_some() {
        let mut pretty = serde_json::to_string_pretty(&generated).expect("schema serializes");
        pretty.push('\n');
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|e| {
                panic!(
                    "failed to create snapshot directory {}: {e}",
                    parent.display()
                )
            });
        }
        std::fs::write(&path, pretty)
            .unwrap_or_else(|e| panic!("failed to write blessed snapshot {}: {e}", path.display()));
        eprintln!("blessed {}", path.display());
        return;
    }

    let snapshot_text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "schema snapshot {} is unreadable ({e}). Re-bless deliberately with \
             `TERM_PROOF_BLESS_SCHEMA=1 cargo test -p termproof --test schema_snapshot` \
             and commit the new snapshot.",
            path.display()
        )
    });
    let snapshot: serde_json::Value = serde_json::from_str(&snapshot_text)
        .unwrap_or_else(|e| panic!("snapshot {} is not valid JSON: {e}", path.display()));

    assert_eq!(
        snapshot,
        generated,
        "generated recipe schema drifted from the checked-in snapshot at {} \
         (first difference: {}). If this change is intended, re-bless \
         deliberately with `TERM_PROOF_BLESS_SCHEMA=1 cargo test -p termproof \
         --test schema_snapshot` and commit the new snapshot.",
        path.display(),
        first_difference_path(&snapshot, &generated)
            .map(|p| if p.is_empty() {
                "(root)".to_string()
            } else {
                p
            })
            .unwrap_or_else(|| "(none)".to_string()),
    );
}

#[cfg(test)]
mod helper_tests {
    use super::first_difference_path;
    use serde_json::json;

    #[test]
    fn equal_values_have_no_difference() {
        assert_eq!(
            first_difference_path(&json!({"a": 1, "b": [1, 2]}), &json!({"a": 1, "b": [1, 2]})),
            None
        );
    }

    #[test]
    fn object_key_order_is_not_a_difference() {
        assert_eq!(
            first_difference_path(&json!({"a": 1, "b": 2}), &json!({"b": 2, "a": 1})),
            None
        );
    }

    #[test]
    fn keyword_change_is_a_difference() {
        assert_eq!(
            first_difference_path(&json!({"definitions": {}}), &json!({"$defs": {}})),
            Some("/$defs".to_string())
        );
    }

    #[test]
    fn array_order_is_a_difference() {
        assert_eq!(
            first_difference_path(
                &json!({"required": ["name", "command"]}),
                &json!({"required": ["command", "name"]})
            ),
            Some("/required/0".to_string())
        );
    }

    #[test]
    fn number_shape_is_a_difference() {
        assert_eq!(
            first_difference_path(&json!({"minimum": 0.0}), &json!({"minimum": 0})),
            Some("/minimum".to_string())
        );
    }

    #[test]
    fn nested_difference_reports_the_full_path() {
        assert_eq!(
            first_difference_path(
                &json!({"properties": {"recipe_version": {"const": 1}}}),
                &json!({"properties": {"recipe_version": {"const": 2}}}),
            ),
            Some("/properties/recipe_version/const".to_string())
        );
    }
}
