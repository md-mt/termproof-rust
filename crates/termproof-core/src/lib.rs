//! TermProof core: models, config, schema, registries, planning, and
//! orchestration.
//!
//! This crate is the shared foundation for the Rust reimplementation. During
//! the RUST-002 baseline it only carried the canonical identity constants; as
//! of RUST-007 it also owns `StepResult` and the seven built-in steps.

pub mod models;
pub mod steps;

/// Canonical product name used by the CLI and diagnostics.
pub const NAME: &str = "termproof";

/// Canonical crate/product version, inherited from the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Render the canonical `name version` banner used by the CLI greeting.
pub fn banner() -> String {
    format!("{NAME} {VERSION} (rust workspace baseline)")
}
