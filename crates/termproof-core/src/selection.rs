//! Choosing which recipes to run for a set of changed files.
//!
//! Running a whole suite on every change is the honest default and, past a
//! certain size, the reason nobody runs it. This maps a changeset onto the
//! recipes that could plausibly be affected, using each recipe's `ci_paths`
//! globs.
//!
//! Two rules do the work:
//!
//! **Smoke recipes always run.** A changeset that matches nothing still gets a
//! baseline, so "selected nothing" never silently means "verified nothing".
//!
//! **A change to the harness selects only smoke.** If any changed file is under
//! [`SelectionConfig::harness_root`], the change is to the verifier rather than
//! to what it verifies, and matching individual `ci_paths` would be noise. One
//! such path in a changeset suppresses every other match in it.
//!
//! ```
//! use termproof_core::selection::{select, SelectionConfig};
//! # use termproof_core::recipe::Recipe;
//! # fn recipe(name: &str, paths: &[&str]) -> Recipe {
//! #     serde_json::from_value(serde_json::json!({
//! #         "name": name, "ci_paths": paths, "steps": [],
//! #         "command": {"argv": ["true"]}
//! #     })).expect("valid recipe")
//! # }
//! let recipes = vec![
//!     recipe("smoke", &[]),
//!     recipe("payments", &["src/payments/**"]),
//!     recipe("search", &["src/search/**"]),
//! ];
//! let config = SelectionConfig {
//!     harness_root: "verify/",
//!     repo_marker: "/repo/",
//!     smoke: &["smoke"],
//! };
//!
//! let picked = select(&recipes, &["src/payments/api.rs".to_string()], &config);
//! let names: Vec<&str> = picked.iter().map(|r| r.name.as_str()).collect();
//! assert_eq!(names, ["smoke", "payments"]);
//! ```

use std::collections::HashSet;

use globset::GlobBuilder;
use globset::GlobSetBuilder;

use crate::recipe::Recipe;

/// Facts about one repository's layout that selection needs.
///
/// All three are the caller's to supply: they describe a checkout, not
/// verification.
pub struct SelectionConfig<'a> {
    /// Repo-relative prefix where the verification harness itself lives.
    ///
    /// A change under this prefix means the harness changed rather than the
    /// thing it verifies, so only [`SelectionConfig::smoke`] runs.
    pub harness_root: &'a str,

    /// Path segment marking the repository root inside an absolute path.
    ///
    /// Changed-file lists arrive with whatever prefix the caller's checkout
    /// has; everything up to and including this marker is stripped so paths
    /// compare repo-relative. One repo's `/src/` is another's `/code/`.
    pub repo_marker: &'a str,

    /// Recipes that run for every changeset, whatever it touched.
    pub smoke: &'a [&'a str],
}

/// Read newline-delimited changed files, dropping blank lines.
pub fn changed_files_from_file(path: &str) -> std::io::Result<Vec<String>> {
    let contents = std::fs::read_to_string(path)?;
    Ok(contents
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

/// The recipes to run for `changed_files`, in their original order.
pub fn select<'a>(
    recipes: &'a [Recipe],
    changed_files: &[String],
    config: &SelectionConfig<'_>,
) -> Vec<&'a Recipe> {
    let mut chosen: HashSet<&str> = config
        .smoke
        .iter()
        .filter(|name| recipes.iter().any(|r| r.name == **name))
        .copied()
        .collect();

    let paths: Vec<String> = changed_files
        .iter()
        .map(|p| normalize_path(p, config.repo_marker))
        .collect();

    let touches_harness = paths.iter().any(|p| p.starts_with(config.harness_root));
    if !touches_harness {
        for r in recipes {
            if matches_any_path(&r.ci_paths, &paths, config.repo_marker) {
                chosen.insert(r.name.as_str());
            }
        }
    }

    recipes
        .iter()
        .filter(|r| chosen.contains(r.name.as_str()))
        .collect()
}

/// Names of the recipes [`select`] would run.
pub fn select_names(
    recipes: &[Recipe],
    changed_files: &[String],
    config: &SelectionConfig<'_>,
) -> Vec<String> {
    select(recipes, changed_files, config)
        .into_iter()
        .map(|r| r.name.clone())
        .collect()
}

/// Reduce a path to a repo-relative, forward-slashed form.
fn normalize_path(path: &str, repo_marker: &str) -> String {
    let mut normalized = path.trim().replace('\\', "/");
    while let Some(rest) = normalized.strip_prefix("./") {
        normalized = rest.to_string();
    }
    // Everything up to *and including* the marker goes: the marker names the
    // checkout directory, which is not part of a repo-relative path. Keeping
    // it would leave `repo/src/foo.rs`, which no `ci_paths` pattern matches.
    if let Some(idx) = normalized.find(repo_marker) {
        normalized = normalized[idx + repo_marker.len()..].to_string();
    }
    normalized.trim_end_matches('/').to_string()
}

/// Whether any of `paths` matches any of `patterns`.
///
/// `literal_separator(false)` so `*` crosses `/`, matching `fnmatch` rather
/// than shell globbing. A pattern that does not compile is skipped rather than
/// failing the selection: a typo in one recipe's `ci_paths` should not stop the
/// suite from choosing the others.
fn matches_any_path(patterns: &[String], paths: &[String], repo_marker: &str) -> bool {
    if patterns.is_empty() || paths.is_empty() {
        return false;
    }
    let mut builder = GlobSetBuilder::new();
    let mut any = false;
    for pattern in patterns {
        if let Ok(glob) = GlobBuilder::new(&normalize_path(pattern, repo_marker))
            .literal_separator(false)
            .build()
        {
            builder.add(glob);
            any = true;
        }
    }
    if !any {
        return false;
    }
    match builder.build() {
        Ok(set) => paths.iter().any(|p| set.is_match(p)),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build through serde rather than a struct literal: `Recipe` has many
    /// `#[serde(default = "...")]` fields, and a literal would hard-code my
    /// guesses at their values instead of using theirs.
    fn recipe(name: &str, paths: &[&str]) -> Recipe {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "ci_paths": paths,
            "steps": [],
            // `command` is the one required field with no serde default.
            // Selection never reads it, but a `Recipe` cannot exist without one.
            "command": {"argv": ["true"]},
        }))
        .expect("valid recipe")
    }

    fn all() -> Vec<Recipe> {
        vec![
            recipe("smoke", &[]),
            recipe("payments", &["src/payments/**"]),
            recipe("search", &["src/search/**/*.rs"]),
        ]
    }

    fn cfg() -> SelectionConfig<'static> {
        SelectionConfig {
            harness_root: "verify/",
            repo_marker: "/repo/",
            smoke: &["smoke"],
        }
    }

    fn names(changed: &[&str]) -> Vec<String> {
        let owned: Vec<String> = changed.iter().map(|s| s.to_string()).collect();
        select_names(&all(), &owned, &cfg())
    }

    #[test]
    fn smoke_runs_even_when_nothing_matches() {
        // "Selected nothing" must never quietly mean "verified nothing".
        assert_eq!(names(&["docs/readme.md"]), vec!["smoke".to_string()]);
    }

    #[test]
    fn a_matching_path_adds_its_recipe() {
        assert_eq!(
            names(&["src/payments/api.rs"]),
            vec!["smoke".to_string(), "payments".to_string()]
        );
    }

    #[test]
    fn a_harness_change_suppresses_every_other_match() {
        // The change is to the verifier, not to what it verifies. One such path
        // in a changeset suppresses the rest.
        assert_eq!(
            names(&["src/payments/api.rs", "verify/runner.rs"]),
            vec!["smoke".to_string()]
        );
    }

    #[test]
    fn absolute_paths_are_made_repo_relative() {
        assert_eq!(
            names(&["/home/someone/repo/src/search/index.rs"]),
            vec!["smoke".to_string(), "search".to_string()]
        );
    }

    #[test]
    fn windows_separators_and_dot_prefixes_normalise() {
        assert_eq!(
            names(&[".\\src\\payments\\api.rs"]),
            vec!["smoke".to_string(), "payments".to_string()]
        );
    }

    #[test]
    fn a_star_crosses_directory_separators() {
        // fnmatch semantics, not shell globbing: `src/search/**/*.rs` has to
        // match a file several directories down.
        assert_eq!(
            names(&["src/search/backend/index/mod.rs"]),
            vec!["smoke".to_string(), "search".to_string()]
        );
    }

    #[test]
    fn a_broken_glob_does_not_stop_the_others() {
        let recipes = vec![
            recipe("smoke", &[]),
            recipe("broken", &["["]),
            recipe("payments", &["src/payments/**"]),
        ];
        let changed = vec!["src/payments/api.rs".to_string()];
        let got = select_names(&recipes, &changed, &cfg());
        assert!(got.contains(&"payments".to_string()), "{got:?}");
    }

    #[test]
    fn a_smoke_name_that_does_not_exist_is_ignored() {
        let config = SelectionConfig {
            harness_root: "verify/",
            repo_marker: "/repo/",
            smoke: &["smoke", "nonexistent"],
        };
        let changed = vec!["docs/readme.md".to_string()];
        assert_eq!(
            select_names(&all(), &changed, &config),
            vec!["smoke".to_string()]
        );
    }

    #[test]
    fn selection_preserves_the_input_order() {
        // Callers report on this list; a set-ordered result would reshuffle
        // reports between runs for no reason.
        let changed = vec![
            "src/search/index.rs".to_string(),
            "src/payments/api.rs".to_string(),
        ];
        assert_eq!(
            select_names(&all(), &changed, &cfg()),
            vec![
                "smoke".to_string(),
                "payments".to_string(),
                "search".to_string()
            ]
        );
    }

    #[test]
    fn an_empty_changeset_still_runs_smoke() {
        assert_eq!(select_names(&all(), &[], &cfg()), vec!["smoke".to_string()]);
    }
}
