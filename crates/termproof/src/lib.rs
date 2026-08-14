//! TermProof: evidence-first verification for TUI and terminal applications.
//!
//! # Layout
//!
//! This crate was merged from three (`termproof-core`, `termproof-terminal`,
//! `termproof-evidence`) before any of them was published, so the shape below
//! is the only one that has ever existed on crates.io.
//!
//! - **The crate root** is what was `termproof-core`: the recipe model,
//!   config, schema, validation, the built-in [`steps`] and [`assertions`],
//!   [`planner`]/[`runner`]/[`execution`], [`store`]/[`cache`], and the
//!   `py*` compatibility shims that keep this port close to the Python
//!   oracle. It is flat rather than under a `core` module, both because it is
//!   the crate's primary surface and because a module named `core` shadows the
//!   `core` crate for every path in its scope.
//! - **[`terminal`]** is what was `termproof-terminal`: PTY, tmux and process
//!   sessions, plain and attributed screen state, asciicast recording, idle
//!   detection and the [`terminal::SessionBackend`] implementations.
//! - **[`evidence`]** is what was `termproof-evidence`: screenshot and video
//!   rendering, Markdown and JUnit reports, visual baselines, diff and upload.
//!
//! The two nested modules keep their own re-exports rather than flattening
//! into the root. `error` is defined by both the root and [`terminal`], so
//! nesting is what keeps [`crate::error`] and [`terminal::error`] apart; the
//! same nesting keeps [`crate::result`] clear of [`evidence::report`], and
//! keeps the reader's sense of which layer a name comes from.
//!
//! Original module notes: models, config, schema, registries, planning,
//! orchestration and execution (RUST-004 + RUST-010 + RUST-016); PTY/process
//! ownership, terminal screen, cast recording, idle and session backends
//! (RUST-005/006 + RUST-012 + RUST-016); and the evidence pipeline.
//!
//! # Features
//!
//! Both are on by default, so a consumer that does not name features gets the
//! whole crate — the shape that has always been published.
//!
//! - **`evidence`** — the [`evidence`] module. Off, the crate does not compile
//!   `image`, `quick-junit` or `avt`.
//! - **`json-schema`** — [`validation`], [`pyschema`] and the `json_schema`
//!   built-in assertion. Off, the crate does not compile `jsonschema`, and
//!   `json_schema` is absent from [`assertions::BUILTIN_TYPES`] rather than
//!   present and failing. Schema *generation* ([`schema`]) is unconditional.
//!
//! [`terminal`] has no feature of its own: the crate root is built on it, and
//! every build drives a terminal, so there is nothing to save.

#[cfg(feature = "evidence")]
pub mod evidence;
pub mod terminal;

pub mod agent;
pub mod assertions;
pub mod before_after;
pub mod build_info;
pub mod cache;
pub mod config;
pub mod error;
pub mod execution;
pub mod models;
pub mod parity;
pub mod planner;
pub mod pypath;
pub mod pyregex;
pub mod pyrepr;
#[cfg(feature = "json-schema")]
pub mod pyschema;
pub mod recipe;
pub mod result;
pub mod run_config;
pub mod runner;
pub mod schema;
pub mod selection;
pub mod steps;
pub mod store;
#[cfg(feature = "json-schema")]
pub mod validation;
pub mod vocabulary;

// Re-exports: config + recipe/schema/validation (RUST-004)
pub use config::VerifierConfig;
pub use recipe::{Assertion, CommandSpec as RecipeCommandSpec, Recipe, Step, RECIPE_VERSION};
#[cfg(feature = "json-schema")]
pub use validation::{has_errors, Severity, ValidationIssue};

// Re-exports: models/result/store (RUST-010) — models is legacy, result is canonical
// Canonical Recipe is from recipe.rs (serde+schemars); models::Recipe retained as ModelRecipe for back-compat
pub use models::Recipe as ModelRecipe;
pub use models::{
    AssertionResult as ModelAssertionResult, CommandSpec as ModelCommandSpec,
    RunResult as ModelRunResult, StepResult as ModelStepResult,
};
// Canonical RunResult/AssertionResult/StepResult from result.rs (score_from_assertions, BTreeMap artifacts)
pub use result::{AssertionResult, RunResult, StepResult};
pub use store::{
    atomic_write, atomic_write_text, ensure_within_base, new_run_dir, sanitize_component,
};

// Re-exports: execution/agent (RUST-016/017)
pub use agent::{
    build_agent_prompt, parse_agent_output, ParsedAgentOutput, MAX_AGENT_OUTPUT_BYTES,
    MAX_PROMPT_CONTEXT_CHARS,
};
pub use error::CoreError;
pub use execution::{
    AgentDrivenMode, ExecutionContext, ExecutionError, ExecutionMode, ExecutionResult,
    ScriptedProcessMode, ScriptedPtyMode,
};
pub use runner::{LoadedRecipe, Runner};

/// Canonical product name used by the CLI and diagnostics.
pub const NAME: &str = "termproof";

/// Canonical crate/product version, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Render the canonical `name version` banner used by the CLI greeting.
pub fn banner() -> String {
    format!("{NAME} {VERSION} (rust workspace baseline)")
}
