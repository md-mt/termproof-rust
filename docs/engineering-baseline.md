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
- A comment in `Cargo.toml` records the reason but does not publish it. The
  manifest is not what a consumer weighing the dependency reads — crates.io
  renders the crate README and docs.rs renders the module — so a floor above
  the oldest workable version is also stated in `crates/termproof/README.md`,
  and in the module whose API pins it where there is one. #35 was filed
  because two such floors were reasoned about only in the manifest and read
  from outside as arbitrary. A floor is only as good as the argument a
  consumer can find for it.
- The reason must be the true one. "This version is required" and "the older
  version would need a different implementation with a weaker guarantee" are
  different claims, and the second is often the accurate one; it is written
  that way. A floor kept for a reason that turns out to be a preference is
  still allowed, but it says so.
- The dependency/feature graph is kept minimal: default features are enabled
  only when needed, and optional heavy dependencies (ffmpeg/agg adapters,
  Docker) are behind features or adapter traits, never hard required.
- Dependency/license/advisory checks are part of the CI gate (RUST-003) with
  documented exceptions; new dependency added in RUST-007 is
  `regex` 1.13.1 (corpus-selected engine per spec §5.3, declared as
  `default-features = false` with explicit `std`+`unicode` features).
- Since PR4, `cargo deny check` is a required gate
  (`.github/workflows/security.yml`), covering advisories, licences, bans and
  sources against `deny.toml`. It runs on every pull request, on push to
  `main`, and weekly on a schedule (new RustSec advisories land without a code
  change; the weekly run re-reads the committed lockfile against a fresh
  database). Intentional exceptions live in `deny.toml` and must carry an
  owner and, where meaningful, an expiry condition — see the file. Duplicate
  versions are *denied* (`bans.multiple-versions = "deny"`), so any duplicate
  that has not been explicitly reviewed and skipped fails the gate. The
  current graph carries eight reviewed skips, each with an inline rationale
  grounded in the dependency tree: `bitflags` 1.3.2 and `thiserror`/`thiserror-impl`
  1.0.69 (both pinned by `portable-pty` 0.9.0), `hashbrown` 0.12.3 and
  `indexmap` 1.9.3 (both via `schemars` 0.8.22), `syn` 2.0.119 (the older
  derive stack: schemars, ICU zerovec/yoke, thiserror-impl 1.x, wasm/windows
  tooling), `unicode-width` 0.1.14 via `avt` beside the 0.2.2 floor (a
  permanent exception, documented in `Cargo.toml` and `terminal::attributed`),
  and `vte` 0.14.1 via `strip-ansi-escapes` beside 0.15.0 via `vt100`
  (incidental; it is what the bump of `quick-junit` to 0.7 carries and is not
  worth a suppression). A new duplicate is a gate failure, not a warning.
- Supply-chain hygiene for the automation itself is part of the baseline:
  every third-party GitHub Action is pinned to an immutable commit SHA with a
  version comment (see §11), and `.github/dependabot.yml` opens weekly grouped
  update PRs for both Cargo and GitHub Actions. A pin without a comment is a
  pin that cannot be updated safely.

## 7. Feature policy

- Cargo features are additive only. Turning a feature on must never change the
  behavior of the code compiled without it.
- Feature names are documented in the crate `Cargo.toml` with a one-line
  description and the issue that justifies them.
- No feature silently changes CLI defaults or result semantics; behavior
  changes land behind explicit flags or features with tests.
- `termproof` has four, all default-on. From #27: `evidence` (the `evidence`
  module — `image`, `avt`) and `json-schema` (`validation`, `pyschema` and the
  `json_schema` built-in assertion — `jsonschema`). From #28: `schema` (the
  `schema` module and the derived `JsonSchema` impls — `schemars`), which
  `json-schema` implies, since validating a recipe means validating it against
  the schema `schema` generates. From #34: `junit` (the `junit` module —
  `quick-junit`). Default is the whole crate; `default-features = false` is 66
  transitive dependencies against 180.
- A feature may exist for reasons other than compile cost. `schema` is six
  crates and would not earn a gate on size; it has one because `schemars` is
  the only dependency that reaches the public API, so it is the only one a
  consumer cannot deduplicate by pinning. Turning the feature off is the
  non-breaking answer to a consumer on a different `schemars` major (#28).
- A feature implies another only where the code says it must. `json-schema`
  implies `schema` because there is no other source for the schema it
  validates against. `junit` implies nothing: #34 proposed
  `junit = ["evidence", ...]`, and `generate_junit` turned out to read a
  `RunResult` and nothing else, so the writer moved to a root `junit` module
  and the two features are independent in both directions. An implication that
  the code does not require is a dependency a consumer pays for and cannot
  decline.
- Gates go at module boundaries. Two exceptions, both contained: `assertions`
  has one `#[cfg]` match arm in `dispatch` plus one contiguous block of five
  private functions serving `json_schema` and nothing else; `schema` reaches
  into `recipe.rs` and `config.rs` as seven `cfg_attr` derives and fifteen
  `cfg_attr` helper attributes, which is unavoidable because `#[schemars(...)]`
  is an inert attribute registered by the derive and will not compile without
  it. A derive that lands on public types cannot be gated at a module boundary.
  `junit` added no third exception: it gates a module. The two `#[cfg(feature =
  "junit")] pub use` lines in `evidence` are path aliases that keep the 0.3.1
  paths resolving — they compile no code of their own.
- Every combination is built and tested, not just `default` and
  `--all-features`. With four features that is the full powerset of sixteen —
  including those that resolve to the same set as another, because what is
  checked is that every combination a consumer can *write* compiles. The CI
  step enumerates the powerset from a feature list rather than spelling the
  combinations out, so adding a feature grows the matrix by construction.
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
    │                           #   rendering, Markdown reports, video, baselines, diff,
    │                           #   cache; `junit` for JUnit XML from a `RunResult`
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
- The generated recipe schema is pinned by a checked-in snapshot
  (`crates/termproof/tests/schema_snapshot.rs` plus
  `crates/termproof/tests/snapshots/recipe_schema_v1.json`). The guard exists
  because the schema unit tests assert only `$schema`, two `required` entries
  and the `recipe_version` const, so a structural rewrite (`definitions` to
  `$defs` with every `$ref` retargeted, `minimum: 0.0` to `minimum: 0`, key
  and `required` ordering) can slip through with the suite green — exactly
  what a `schemars` major bump did (#33).
- The snapshot is compared as parsed `serde_json::Value` trees, not as text:
  object key order in the file is ignored (semantically irrelevant), but every
  structural difference — keywords, numbers, array order, `$ref` targets —
  fails the test. The test and its snapshot ship in the published package, so
  a consumer testing the published crate gets the same check.
- Re-blessing is deliberate and never the default. After an intentional schema
  change, run
  `TERM_PROOF_BLESS_SCHEMA=1 cargo test -p termproof --test schema_snapshot`
  to rewrite the snapshot, review the diff, and commit the new snapshot in the
  same change as the schema edit. The env var name follows the `TERM_PROOF_*`
  convention used elsewhere in the crate.
- What the snapshot does and does not prove: it **does** catch accidental
  changes to this crate's generated schema; it does **not** establish
  agreement with the canonical schema, which lives outside this repository and
  is not vendored here. That remains parity-gate work (the seam is
  `schema::load_canonical_schema`, which returns `None` in every checkout
  layout here today).
- Every Rust pull request must pass locally, before push:
  `cargo fmt --check --all`, `cargo clippy --workspace --all-targets
  --all-features -- -D warnings`, and `cargo test --workspace`. Where the
  change touches dependencies, packaging or the public API, the corresponding
  security-workflow gates (§11) apply as well: `cargo deny check` and
  `cargo package -p termproof` locally, `cargo semver-checks check-release
  -p termproof` before a release changes the public API.

## 11. Supply chain, CI and release verification

- **Immutable action pins.** Every third-party GitHub Action in
  `.github/workflows/` is referenced by commit SHA, with the human-readable
  version in a trailing comment (`actions/checkout@<sha> # v7.0.1`). A moving
  tag (`@v7`, `@master`) can be replaced silently by its owner; a SHA cannot.
  Dependabot moves SHA and comment together on update, which is why the
  comment is mandatory — a SHA without a version cannot be reviewed.
- **Dependabot.** `.github/dependabot.yml` opens weekly grouped PRs for Cargo
  and GitHub Actions, capped at five open PRs per ecosystem. Updates are
  reviewed against the dependency-floor test (§6) and the feature-powerset
  test (§7) in the same PR.
- **cargo deny.** `deny.toml` is the workspace's dependency policy — licence
  allowlist (deny-by-default, and equal to the live graph: an allowance no
  dependency uses is removed, so `cargo deny check` reports no unused-license
  warnings), bans (duplicates *denied* unless explicitly reviewed and skipped;
  wildcards denied), sources (crates.io only), and advisories (every
  vulnerability and unsound advisory fails; `yanked` crates fail). Every
  intentional exception carries an owner and, where meaningful, an expiry
  condition; the eight current duplicate skips each carry an inline reason
  grounded in the dependency tree (§6). Enforced by the `Security` workflow on
  every PR and push, and weekly on a schedule so a freshly published advisory
  is caught without waiting for a PR.
- **cargo semver-checks.** The `Security` workflow compares this crate's
  public API against the latest version published on crates.io on every PR
  (`cargo semver-checks check-release -p termproof`), and fails on a breaking
  change. Under the pre-1.0 convention (docs/publishing.md) that is any
  change the convention would call a minor bump; it is caught here before a
  release, not by the first consumer who fails to compile.
- **Package verification.** PR CI runs `cargo package -p termproof` and
  `.github/scripts/verify-package-contents.sh`, which asserts the tarball
  carries the snapshot test and its fixture (issue #33's promise) and does
  not carry the differential tests or `harness/` (which cannot run without
  the repository).
- **Release archives.** `release-rust.yml` smoke-tests every archive before
  upload: `.github/scripts/verify-release-archive.sh` verifies the sha256,
  extracts the tarball, and asserts `termproof --version` reports exactly the
  workspace version. The attestation subject is verified against the archive
  digest, so a green build-provenance attestation always names the archive it
  was produced for.
- **Container image.** `docker/termproof.Dockerfile` builds
  `ghcr.io/<owner>/termproof-rust` in two stages; the runtime stage carries no
  Rust toolchain, and `docker/smoke/run-smoke.sh` asserts that rather than
  trusting it. Both base images are pinned by digest for the same reason
  actions are pinned by SHA, and `.github/dependabot.yml` moves the tag and
  the digest together. Pull requests build the image, do not push it, and run
  a real recipe inside it — one that drives `rsvg-convert` and `ffmpeg` on a
  pseudo-terminal and asserts on the artifacts, not on the exit code. Nothing
  reaches the registry that has not passed that run.
- **Windows is not supported.** The tested matrix is Linux x86-64, macOS
  x86-64 and macOS arm64 (build-only for arm64). Windows is documented as
  unverified in the README and gets no badge and no claim until a real
  Windows job with working PTY behaviour passes CI. This is a deliberate
  scope decision (spec §3), not an oversight.
- **Stable job names.** The checks a repository ruleset requires are named
  explicitly and stably in every workflow (`fmt, clippy, test (Rust
  ubuntu-latest)`, `cargo deny (advisories, licenses, bans, sources)`, …). A
  ruleset entry that names a job by a name that changes between runs would
  silently stop gating, so workflow job names are part of the contract.
