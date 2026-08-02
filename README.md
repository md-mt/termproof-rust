# TermProof Rust Workspace

This directory holds the Rust reimplementation of TermProof. It is a fully
separate workspace from the Python implementation at the repository root: the
Python package, packaging, CLI default, and docs remain unchanged at the top
level until the parity gates in `docs/rust-reimplementation-spec.md` pass.

## Layout

- `Cargo.toml` — workspace manifest with shared lint and dependency policy.
- `rust-toolchain.toml` — pinned toolchain (`1.96.0`, minimal profile).
- `docs/engineering-baseline.md` — formatting, lint, error, tracing,
  dependency, feature, and unsafe-code policies (RUST-002 deliverable).
- `crates/` — five workspace crates:
  - `termproof-cli` — binary (`termproof`), command parsing, diagnostics
  - `termproof-core` — models, config, schema, registries, planning, orchestration
  - `termproof-terminal` — PTY/process sessions, terminal screen, cast recording
  - `termproof-evidence` — rendering, reports, video, baselines, diff, cache
  - `termproof-plugin-protocol` — versioned process messages, client/host support

## Quickstart

```sh
cd rust

# Run the baseline binary
cargo run -p termproof-cli

# Local gates (must pass before every push; CI enforces the same)
cargo fmt --check --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
cargo deny check licenses advisories bans sources

# Gate scripts (from the repository root):
uv run python -m unittest discover -s rust/scripts/tests -v
uv run python rust/scripts/check_schema_drift.py --canonical docs/recipe-schema-v1.json --rust-root rust
uv run python rust/scripts/check_conformance.py \
  --corpus rust/conformance/corpus.json \
  --binary rust/target/debug/termproof \
  --oracle "uv run python -m termproof"
cargo llvm-cov --workspace --json --output-path /tmp/cov.json
python3 rust/scripts/check_coverage_regression.py \
  --baseline rust/coverage/baseline.json --current /tmp/cov.json
```

## Status

RUST-002 baseline: the workspace builds, lints clean, and the `termproof`
binary prints the canonical greeting. Real behavior lands in the M0–M5
milestones tracked by issues 94–123.
