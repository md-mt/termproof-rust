//! TermProof core: models, config, schema, registries, planning, orchestration, and execution (RUST-004 + RUST-010 + RUST-016).

pub mod agent;
pub mod cache;
pub mod config;
pub mod error;
pub mod execution;
pub mod models;
pub mod planner;
pub mod pyregex;
pub mod pyrepr;
pub mod recipe;
pub mod result;
pub mod runner;
pub mod schema;
pub mod steps;
pub mod store;
pub mod validation;

// Re-exports: config + recipe/schema/validation (RUST-004)
pub use config::VerifierConfig;
pub use recipe::{Assertion, CommandSpec as RecipeCommandSpec, Recipe, Step, RECIPE_VERSION};
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
