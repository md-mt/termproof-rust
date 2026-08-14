# TermProof Rust Engineering Baseline

Status: Active
Owner: termproof maintainers
Scope: this workspace (all five crates)
Applies from: RUST-002 (milestone Rust M0 — Contract and skeleton)

This document is the engineering policy for the Rust reimplementation. It
complements [`docs/rust-reimplementation-spec.md`](rust-reimplementation-spec.md),
which moved here with the port; where the two disagree the specification wins
and this document is updated.

## 1. Toolchain and MSRV

- The workspace pins an exact Rust toolchain via `rust-toolchain.toml`
  (`channel = "1.96.0"`, `profile = "minimal"`, components `rustfmt` and
  `clippy`). The pin is deliberately an exact stable release, not the moving
  `stable` alias, so a fresh environment reproduces the same compiler. CI and
  developers using `rustup` pick this up automatically; Homebrew/manual
  installs must install the same exact channel.
- The minimum supported Rust version (MSRV) is declared as
  `rust-version = "1.96"` in `Cargo.toml` (`[workspace.package]`). The
  pinned toolchain matches the MSRV, so development and CI run on the declared
  minimum. Code must compile on the MSRV with no warnings.
- No nightly-only production features are used. Nightly is allowed only for
  local experimentation and never in committed code or CI.
- Target support policy (Tier 1 at cutover, per spec section 3):
  - Linux x86-64
  - macOS x86-64
  - macOS arm64
  - CI must build and test at least Linux x86-64 and macOS x86-64 on every
    Rust pull request; arm64 is added to the gate when a runner is available.

## 2. Formatting policy

- `rustfmt` with the workspace defaults is the single source of formatting
  truth. There is intentionally no `rustfmt.toml` override in the baseline.
- Every Rust pull request must pass `cargo fmt --check --all`.
- Never commit hand-formatted code that `cargo fmt` would change.
- Rust code does not appear in Python `ruff`/`black` scopes; the Python oracle
  keeps its own formatting policy unchanged.

## 3. Lint and warnings policy

- Workspace lints live in `Cargo.toml` under `[workspace.lints]` and are
  inherited by every crate through `[lints] workspace = true`:
  - `rust.unsafe_code = "forbid"` — no `unsafe` in workspace code (see §8).
  - `rust.missing_docs = "warn"` — public items must be documented.
  - `clippy.all = "warn"` — the default Clippy lint set.
- Every Rust pull request must pass:
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  (warnings denied).
- The CI gate (added in RUST-003) runs this exact command; the local gate is
  the same command and must be green before push.
- New lint suppressions need a code comment explaining why and should be
  scoped as narrowly as possible (`#[allow(...)]` on the item, not the crate).

## 4. Error policy

- Internal errors are typed, thiserror-style enums. A crate defines its own
  error type in its private `error` module and exposes it as needed.
- Typed internal errors are converted at the crate/public boundary into
  stable public diagnostics (the spec's "converted at the boundary into stable
  public diagnostics", section 5.3).
- Failures that are part of normal operation (step failures, assertion
  failures, plugin protocol errors) are structured results, not panics.
- `panic!`, `unwrap`, `expect`, `unreachable!`, and `todo!` are banned in
  production paths that can be reached by user input. Test code may use
  `unwrap`/`expect` for fixture assumptions.
- CLI exit-code semantics are preserved from the Python oracle (frozen in
  RUST-001) and never invented ad hoc.

## 5. Tracing policy

- Structured diagnostics use the `tracing` family of crates (`tracing`,
  `tracing-subscriber`); `println!`/`eprintln!` are not used for diagnostics.
  The single exception is the RUST-002 baseline binary greeting, which is the
  artifact under test.
- Log levels follow the convention: `error` for failures, `warn` for
  recoverable anomalies, `info` for run/step boundaries, `debug` for detail,
  `trace` for byte-level terminal and protocol traffic.
- No secrets, recipe contents, or full command output are logged at `info` or
  below by default; redaction is applied at the boundary.
- CI output must not leak credentials; tokens and keys never appear in logs.

## 6. Dependency policy

- All dependencies are pinned in `rust/Cargo.lock` (committed) and resolved
  with the workspace `resolver = "2"`.
- No dependency is added without a documented reason in the relevant issue
  (spec section 5.3 lists the planned technology choices: serde, serde_json, a
  maintained Serde YAML implementation, schemars, jsonschema, portable-pty,
  vt100, quick-junit, and a corpus-selected regex engine).
- Dependencies are declared once in `[workspace.dependencies]` and referenced
  with `.workspace = true` so versions stay uniform across crates.
- A version requirement is a floor, not a preference: the lowest version the
  code compiles and passes the differential harnesses against, established by
  building at it rather than by reading a changelog. A floor above the oldest
  workable version costs every vendoring consumer a duplicate copy of the
  crate, so one that sits higher than it has to carries a comment in
  `Cargo.toml` naming the API or behaviour that put it there (#28). The
  `test at the declared dependency floors` step in `.github/workflows/rust.yml`
  runs the suite at each floor, so a floor that stops being true fails CI.
- The dependency/feature graph is kept minimal: default features are enabled
  only when needed, and optional heavy dependencies (ffmpeg/agg adapters,
  Docker) are behind features or adapter traits, never hard required.
- Dependency/license/advisory checks are part of the CI gate (RUST-003) with
  documented exceptions; new dependency added in RUST-007 is
  `regex` 1.13.1 (corpus-selected engine per spec §5.3, declared as
  `default-features = false` with explicit `std`+`unicode` features).

## 7. Feature policy

- Cargo features are additive only. Turning a feature on must never change the
  behavior of the code compiled without it.
- Feature names are documented in the crate `Cargo.toml` with a one-line
  description and the issue that justifies them.
- No feature silently changes CLI defaults or result semantics; behavior
  changes land behind explicit flags or features with tests.
- `termproof` has three, all default-on. From #27: `evidence` (the `evidence`
  module — `image`, `quick-junit`, `avt`) and `json-schema` (`validation`,
  `pyschema` and the `json_schema` built-in assertion — `jsonschema`). From
  #28: `schema` (the `schema` module and the derived `JsonSchema` impls —
  `schemars`), which `json-schema` implies, since validating a recipe means
  validating it against the schema `schema` generates. Default is the whole
  crate; `default-features = false` is 66 transitive dependencies against 180.
- A feature may exist for reasons other than compile cost. `schema` is six
  crates and would not earn a gate on size; it has one because `schemars` is
  the only dependency that reaches the public API, so it is the only one a
  consumer cannot deduplicate by pinning. Turning the feature off is the
  non-breaking answer to a consumer on a different `schemars` major (#28).
- Gates go at module boundaries. Two exceptions, both contained: `assertions`
  has one `#[cfg]` match arm in `dispatch` plus one contiguous block of five
  private functions serving `json_schema` and nothing else; `schema` reaches
  into `recipe.rs` and `config.rs` as seven `cfg_attr` derives and fifteen
  `cfg_attr` helper attributes, which is unavoidable because `#[schemars(...)]`
  is an inert attribute registered by the derive and will not compile without
  it. A derive that lands on public types cannot be gated at a module boundary.
- Every combination is built and tested, not just `default` and
  `--all-features`. With three features that is the full powerset of eight —
  including the two that resolve to the same set, because what is checked is
  that every combination a consumer can *write* compiles.
- A feature must not be able to compile a differential harness out. Parity
  evidence is the reason the harnesses exist, and a combination that drops it
  still reports green. Where a feature genuinely removes the capability a case
  measures, the harness skips those cases *by type*, prints the count, and
  asserts it exactly, with its own floor for the corpus that remains.

## 8. Unsafe-code policy

- `unsafe_code = "forbid"` is enforced workspace-wide by lint; there is no
  `unsafe` in the baseline and none is expected.
- Rust's `forbid` level cannot be lowered by a nested `#[allow(unsafe_code)]`,
  so a scoped exception is not possible without changing the lint itself. If a
  future milestone proves a genuine need for `unsafe` (for example a narrow
  FFI shim), it requires a reviewed workspace-policy change that:
  1. Justifies the need in the implementing issue with a soundness argument;
  2. Relaxes the workspace lint from `forbid` to `deny` in the same reviewed
     change (documented here), so a narrow override becomes possible at all;
  3. Isolates every `unsafe` block in the smallest possible module with a
     safety contract documented in a `// SAFETY:` comment on every block;
  4. Scopes any `#[allow(unsafe_code)]` to the item or module only, never
     crate-wide — `deny` remains the workspace default.
- ABI-stable dynamic libraries are explicitly a non-goal (spec section 4.5);
  `unsafe` for plugin FFI is not a planned use.

## 9. Workspace layout and crate map

```
.
├── Cargo.toml                  # workspace manifest + shared lints/deps
├── rust-toolchain.toml         # pinned toolchain 1.96.0 (exact stable release)
├── Cargo.lock                  # pinned dependency graph (committed)
├── README.md                   # quickstart for the Rust workspace
├── docs/
│   └── engineering-baseline.md # this document
└── crates/
    ├── termproof/              # the library: models, config, schema, registries,
    │                           #   planning, orchestration; `terminal` for PTY/process
    │                           #   sessions, screen and cast recording; `evidence` for
    │                           #   rendering, reports, video, baselines, diff, cache
    ├── termproof-cli/          # binary: command parsing, composition, diagnostics
    └── termproof-plugin-protocol/ # versioned process messages, client/host support
```

Spec section 5.2 lists the three library responsibilities as three crates; they
are now three module trees inside `termproof`, which is what the spec's header
records as superseded. A new crate is created only when it has a real
independent boundary; it must be added to `members`, this document, and the
workspace README in the same change.

## 10. Testing policy (baseline)

- Integration tests run the real compiled binary via
  `CARGO_BIN_EXE_termproof` (see `crates/termproof-cli/tests/cli_baseline.rs`).
- Unit tests live next to the code (`#[cfg(test)]`); integration tests live in
  `tests/`.
- Golden, property, conformance, and packaging smoke tests are added by the
  milestones that need them (spec section 6.1); the baseline keeps the suite
  small and deterministic.
- Every Rust pull request must pass locally, before push:
  `cargo fmt --check --all`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo test --workspace`.
