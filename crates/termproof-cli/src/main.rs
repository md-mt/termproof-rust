//! TermProof command-line entry point.

mod cli;
mod run;

use clap::ArgMatches;
use std::path::PathBuf;

fn main() {
    let code = run();
    std::process::exit(code);
}

fn run() -> i32 {
    let mut cmd = cli::build_cli();
    // Print banner for --version is handled by clap automatically.
    let matches = match cmd.try_get_matches_from_mut(std::env::args()) {
        Ok(m) => m,
        Err(e) => {
            // clap already prints to stderr; exit with usage code 2 for
            // invalid invocation, 0 for --help/--version.
            let _ = e.print();
            return if e.exit_code() == 0 {
                cli::exit_code::SUCCESS
            } else {
                cli::exit_code::USAGE
            };
        }
    };

    match matches.subcommand() {
        Some(("run", sub)) => handle_run(sub),
        Some(("list", sub)) => handle_list(sub),
        Some(("validate", sub)) => handle_validate(sub),
        Some(("plugins", sub)) => handle_plugins(sub),
        Some(("init", sub)) => handle_init(sub),
        Some(("demo", sub)) => handle_demo(sub),
        _ => {
            // No subcommand: preserve RUST-002 baseline greeting on stdout so
            // the existing integration test can migrate, then hint help.
            println!("{}", termproof_core::banner());
            eprintln!("Run `termproof --help` for usage.");
            cli::exit_code::USAGE
        }
    }
}

fn handle_run(m: &ArgMatches) -> i32 {
    let parallel = *m.get_one::<u32>("parallel").unwrap_or(&1);
    let skip_unchanged = m.get_flag("skip-unchanged");
    let diff = m.get_flag("diff");
    let update_baselines = m.get_flag("update-baselines");
    if let Err(msg) =
        cli::validate_run_constraints(parallel, skip_unchanged, diff, update_baselines)
    {
        eprintln!("{msg}");
        return cli::exit_code::USAGE;
    }
    run::execute(m, parallel)
}

fn handle_list(m: &ArgMatches) -> i32 {
    let recipes: Vec<PathBuf> = m
        .get_many::<PathBuf>("recipes")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let priority = m.get_one::<String>("priority");
    println!("list: {} path(s) priority={:?}", recipes.len(), priority);
    cli::exit_code::SUCCESS
}

fn handle_validate(m: &ArgMatches) -> i32 {
    let recipes: Vec<PathBuf> = m
        .get_many::<PathBuf>("recipes")
        .map(|v| v.cloned().collect())
        .unwrap_or_default();
    let mut any_missing = false;
    for p in &recipes {
        if !p.exists() {
            eprintln!("no recipe files found: {}", p.display());
            any_missing = true;
        }
    }
    if any_missing {
        return cli::exit_code::FAILURE;
    }
    println!("validate: {} path(s)", recipes.len());
    cli::exit_code::SUCCESS
}

fn handle_plugins(m: &ArgMatches) -> i32 {
    match m.subcommand() {
        Some(("list", sub)) => {
            let config = sub.get_one::<PathBuf>("config");
            println!("plugins list config={config:?}");
            cli::exit_code::SUCCESS
        }
        Some(("search", sub)) => {
            let query = sub.get_one::<String>("query").cloned().unwrap_or_default();
            let registry = sub
                .get_one::<PathBuf>("registry")
                .cloned()
                .unwrap_or_else(|| PathBuf::from("docs/plugins.md"));
            println!(
                "plugins search query={} registry={}",
                query,
                registry.display()
            );
            cli::exit_code::SUCCESS
        }
        Some(("install", sub)) => {
            let name = sub.get_one::<String>("name").cloned().unwrap_or_default();
            let registry = sub
                .get_one::<PathBuf>("registry")
                .cloned()
                .unwrap_or_else(|| PathBuf::from("docs/plugins.md"));
            let dry_run = sub.get_flag("dry-run");
            println!(
                "plugins install name={} registry={} dry_run={}",
                name,
                registry.display(),
                dry_run
            );
            cli::exit_code::SUCCESS
        }
        _ => cli::exit_code::USAGE,
    }
}

fn handle_init(m: &ArgMatches) -> i32 {
    let path = m.get_one::<PathBuf>("path").cloned().unwrap();
    let name = m.get_one::<String>("name").cloned().unwrap();
    let target_command = m.get_one::<String>("command").cloned().unwrap();
    let non_pty = m.get_flag("non-pty");
    let priority = m
        .get_one::<String>("priority")
        .cloned()
        .unwrap_or_else(|| "P2".to_string());
    let cols = *m.get_one::<u32>("cols").unwrap_or(&100);
    let rows = *m.get_one::<u32>("rows").unwrap_or(&30);
    let force = m.get_flag("force");

    let recipe_path = path.join(format!("{name}.recipe.json"));
    if recipe_path.exists() && !force {
        eprintln!("recipe already exists: {}", recipe_path.display());
        return cli::exit_code::FAILURE;
    }
    if let Err(e) = std::fs::create_dir_all(&path) {
        eprintln!("failed to create {}: {e}", path.display());
        return cli::exit_code::FAILURE;
    }
    let argv = shell_words(&target_command);
    let content = format!(
        "{{\"name\":\"{name}\",\"command\":{{\"argv\":{argv:?},\"pty\":{}}},\"priority\":\"{priority}\",\"cols\":{cols},\"rows\":{rows}}}",
        !non_pty
    );
    match std::fs::write(&recipe_path, content) {
        Ok(()) => {
            println!("created recipe: {}", recipe_path.display());
            cli::exit_code::SUCCESS
        }
        Err(e) => {
            eprintln!("failed to write {}: {e}", recipe_path.display());
            cli::exit_code::FAILURE
        }
    }
}

fn handle_demo(m: &ArgMatches) -> i32 {
    let out = m
        .get_one::<PathBuf>("out")
        .cloned()
        .unwrap_or_else(|| PathBuf::from(".termproof/demo"));
    let video = m.get_flag("video");
    let reporter = m
        .get_one::<String>("reporter")
        .cloned()
        .unwrap_or_else(|| "markdown".to_string());
    println!(
        "demo: out={} video={} reporter={}",
        out.display(),
        video,
        reporter
    );
    cli::exit_code::SUCCESS
}

fn shell_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' if !in_single => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            c if c.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    out.push(cur.clone());
                    cur.clear();
                }
            }
            _ => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
