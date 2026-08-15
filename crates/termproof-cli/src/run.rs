//! `termproof run` — discover recipes, execute them, write the evidence.
//!
//! This used to print a summary of its own arguments and exit 0. Everything
//! it now does goes through the same public surfaces a plugin would use:
//! `termproof::Runner` for execution, `termproof::store` for the
//! run directory and atomic writes, and `termproof::evidence::report` plus
//! `termproof::junit` for the reporters.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use clap::parser::ValueSource;
use clap::ArgMatches;

use termproof::evidence::report::{cli_summary, generate_markdown, generate_markdown_single};
use termproof::junit::generate_junit;
use termproof::planner::{plan_items, run_parallel, PlanItem};
use termproof::result::RunResult;
use termproof::runner::{LoadedRecipe, Runner};
use termproof::store;

use crate::cli::exit_code;

/// Suffixes a directory scan treats as recipes.
const RECIPE_SUFFIXES: &[&str] = &[".recipe.json", ".recipe.yaml", ".recipe.yml"];

/// Flags the CLI accepts but does not act on yet. Saying so beats letting a
/// caller believe evidence was produced that was not.
const UNIMPLEMENTED_FLAGS: &[(&str, &str)] = &[
    ("video", "--video"),
    ("diff", "--diff"),
    ("update-baselines", "--update-baselines"),
    ("skip-unchanged", "--skip-unchanged"),
];

/// Run every recipe the arguments select, returning the process exit code.
pub fn execute(m: &ArgMatches, parallel: u32) -> i32 {
    let paths: Vec<PathBuf> = m
        .get_many::<PathBuf>("recipes")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let out = m
        .get_one::<PathBuf>("out")
        .cloned()
        .unwrap_or_else(|| PathBuf::from(".termproof/runs"));

    for (flag, spelling) in UNIMPLEMENTED_FLAGS {
        if m.get_flag(flag) {
            eprintln!("warning: {spelling} is accepted but not implemented yet; ignoring it");
        }
    }

    let files = match discover(&paths) {
        Ok(files) => files,
        Err(msg) => {
            eprintln!("{msg}");
            return exit_code::FAILURE;
        }
    };
    if files.is_empty() {
        eprintln!("no recipe files found");
        return exit_code::FAILURE;
    }

    let renderer_filter = (m.value_source("renderer") == Some(ValueSource::CommandLine))
        .then(|| m.get_one::<String>("renderer").cloned())
        .flatten();
    let priority_filter = m.get_one::<String>("priority").cloned();
    let name_filter: Vec<String> = m
        .get_many::<String>("recipe-name")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();

    let mut loaded: HashMap<PathBuf, LoadedRecipe> = HashMap::new();
    let mut planned = Vec::new();
    for file in files {
        let recipe = match LoadedRecipe::from_file(&file) {
            Ok(recipe) => recipe,
            Err(e) => {
                eprintln!("{e}");
                return exit_code::FAILURE;
            }
        };
        if let Some(priority) = &priority_filter {
            if &recipe.recipe.priority != priority {
                continue;
            }
        }
        if !name_filter.is_empty() && !name_filter.contains(&recipe.recipe.name) {
            continue;
        }
        let renderers: Vec<(String, Vec<String>)> = match &renderer_filter {
            Some(wanted) => recipe
                .renderers
                .iter()
                .filter(|(name, _)| name == wanted)
                .cloned()
                .collect(),
            None => recipe.renderers.clone(),
        };
        if renderers.is_empty() {
            continue;
        }
        planned.push((file.clone(), recipe.recipe.name.clone(), renderers));
        loaded.insert(file, recipe);
    }

    let items = plan_items(planned);
    if items.is_empty() {
        eprintln!("no recipe files matched the given filters");
        return exit_code::FAILURE;
    }

    let workers = parallel.max(1) as usize;
    let outcomes = run_parallel(items, workers, |item: &PlanItem| {
        let recipe = &loaded[&item.recipe_path].recipe;
        let run_dir = store::new_run_dir(&out, &item.recipe_name, &item.renderer);
        Runner::new()
            .run(recipe, &item.renderer, &run_dir)
            .map(|result| (run_dir, result))
            .map_err(|e| format!("{}: {e}", item.recipe_path.display()))
    });

    let mut results: Vec<RunResult> = Vec::new();
    let mut refused = false;
    for outcome in outcomes {
        match outcome {
            Ok((run_dir, result)) => {
                let report = generate_markdown_single(&result);
                if let Err(e) = store::write_result_files(&run_dir, &result, &report) {
                    eprintln!("failed to write evidence to {}: {e}", run_dir.display());
                    refused = true;
                }
                results.push(result);
            }
            Err(msg) => {
                eprintln!("{msg}");
                refused = true;
            }
        }
    }

    if !results.is_empty() {
        let aggregate = generate_markdown(&results);
        if let Err(e) = store::write_latest_report(&out, &aggregate, ".md") {
            eprintln!("failed to write latest-report.md: {e}");
        }
        if let Some(xml_path) = m.get_one::<PathBuf>("xml-path") {
            let junit = generate_junit(&results);
            if let Some(parent) = xml_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = store::atomic_write_text(xml_path, &junit) {
                eprintln!("failed to write {}: {e}", xml_path.display());
            }
        }
        println!("{}", cli_summary(&results));
    }

    if refused || !results.iter().all(|r| r.passed) {
        exit_code::FAILURE
    } else {
        exit_code::SUCCESS
    }
}

/// Expand the positional arguments into recipe files.
///
/// A file is taken as given; a directory is scanned recursively for the
/// recipe suffixes, sorted so the plan does not depend on the order the
/// filesystem happens to return. A path that does not exist is an error
/// naming it, not a silent zero-recipe success.
fn discover(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            collect_dir(path, &mut files)?;
        } else {
            return Err(format!("no recipe files found: {}", path.display()));
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_dir(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            collect_dir(&child, files)?;
        } else if is_recipe_file(&child) {
            files.push(child);
        }
    }
    Ok(())
}

fn is_recipe_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    RECIPE_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix) && name.len() > suffix.len())
}
