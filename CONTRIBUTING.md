# Contributing to termproof-rust

Thanks for considering a contribution. Please read this before opening an
issue or a pull request — a few minutes here saves both of us a review round.

## What this repository is

`termproof-rust` is the **Rust reimplementation** of
[TermProof](https://github.com/md-mt/termproof). The Python implementation at
[`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
and the behavioural oracle; this port is **in progress** and is **not at
parity** with it. There is no parity gate.

Before you contribute, read:

- the [maturity section of the README](README.md#maturity--read-this-before-using-it) —
  what this port can and cannot do, in plain terms;
- [`docs/rust-reimplementation-spec.md`](docs/rust-reimplementation-spec.md) —
  the design rationale, compatibility contract and parity gates the port is
  measured against;
- [`docs/engineering-baseline.md`](docs/engineering-baseline.md) — the
  workspace policy on formatting, linting, errors, tracing, dependencies,
  features and unsafe code. CI enforces it, and a contribution that fights it
  will not merge.

If your change touches a step or an assertion, also read
[`harness/README.md`](harness/README.md) — the differential harness is how this
port stays honest, and it has rules about regenerating expectations.

## Ground rules

1. **This is a port, not a rewrite.** The Python implementation is the
   behavioural authority. A divergence from it is a *parity gap* to be
   measured and documented, not a stylistic preference to be shipped silently.
   Use the [parity gap issue form](.github/ISSUE_TEMPLATE/parity_gap.yml).
2. **No claim of parity without a measurement.** The README's capability
   table and [`docs/status-and-parity.md`](docs/status-and-parity.md) quote
   the differential harness counts (currently 82/115 full agreement on the
   step corpus, 124/147 on the assertion corpus — see
   [`harness/README.md`](harness/README.md) for the exact numbers and what
   they mean). If your change alters behaviour, update those descriptions
   *and* the counts honestly. A number nobody can reproduce is not a
   measurement.
3. **Version and release claims must be true.** `termproof` is published on
   crates.io (through `0.3.2`); `termproof-cli` and
   `termproof-plugin-protocol` are held back (`publish = false`) and are not
   on the registry. Releases are GitHub Releases cut by
   `.github/workflows/auto-release.yml`, with the `termproof` crate published
   from those releases by `.github/workflows/publish-crates.yml`. See
   [`docs/publishing.md`](docs/publishing.md) for the tag format, the
   version-bump rule and the publish set.

## Getting started

The workspace pins an exact toolchain via `rust-toolchain.toml`
(`channel = "1.96.0"`, `profile = "minimal"`, components `rustfmt` and
`clippy`). With `rustup` installed, the pin is picked up automatically on
first build:

```sh
cargo build --workspace
```

The gates CI enforces on every pull request (`.github/workflows/rust.yml`):

```sh
cargo fmt --check --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Run all three before pushing. CI also runs the suite across the whole feature
powerset (four features — `evidence`, `junit`, `json-schema`, `schema` — so
sixteen combinations) and at the declared dependency floors, so a change that
only breaks a non-default combination will be caught there even if your local
default build is green.

## Finding work

- Issues labelled [`good first issue`](https://github.com/md-mt/termproof-rust/labels/good%20first%20issue)
  are a good starting point.
- Issues labelled [`help wanted`](https://github.com/md-mt/termproof-rust/labels/help%20wanted)
  are open asks.
- Nothing else is off-limits — but if an issue is unlabelled and unassigned,
  comment on it before starting, so two people do not build the same thing.

## Reporting bugs

Use the [bug report form](.github/ISSUE_TEMPLATE/bug_report.yml). It asks for
the version, the platform and a reproduction recipe; all three matter here,
because the port's behaviour depends on the pty backend, the terminal
emulator and the recipe. If the behaviour differs from what the Python
implementation does, use the [parity gap form](.github/ISSUE_TEMPLATE/parity_gap.yml)
instead.

## Proposing features

Use the [feature request form](.github/ISSUE_TEMPLATE/feature_request.yml).
Remember the port's contract: a feature that changes observed behaviour is
measured against the oracle, and the README maturity section must stay
accurate.

## Development workflow

1. **Branch.** This repository has used `wt/<topic>` and `fm/<topic>` branch
   names for worktrees and feature branches; any descriptive name is fine.
   Never commit directly to `main`.
2. **Commit.** Use [Conventional Commits](https://www.conventionalcommits.org/)
   (`feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, ...). The auto-release
   workflow derives version bumps from commit messages, so a misleading type
   is not just cosmetic. A breaking change must carry `!` (or a `BREAKING
   CHANGE:` footer) — under the pre-1.0 rule it is what bumps the minor digit.
3. **Open a pull request** against `main` using the
   [pull request template](.github/PULL_REQUEST_TEMPLATE.md). Keep the PR
   focused; the review is faster and the changelog entry is easier to write.
4. **Update the changelog.** If the change is user-facing, add an entry under
   `[Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md), in the same PR.
5. **Wait for CI, and keep it green.** The `Rust` workflow runs on every pull
   request. Fix failures rather than working around them.

### If you change a step or assertion

The differential harness replays the checked-in corpus through both runtimes
and asserts a floor, not equality. If your change moves the port's answers:

- run `cargo test -p termproof --test differential_steps -- --nocapture`
  (and the assertions twin) and read what it prints;
- update the counts and the divergence list in `harness/README.md` and the
  README maturity section to match;
- do **not** regenerate the recorded oracle expectations to make the test
  pass — `harness/README.md` explains why that is falsifying the measurement.

### If you change a manifest

- Every dependency in `[workspace.dependencies]` carries a documented reason in
  `docs/engineering-baseline.md`; add one for anything new, and name the API
  that justifies a floor above the oldest workable version.
- A requirement here is a *floor*, not a preference. If you widen one, CI pins
  it and tests the suite at it — make sure the code actually works there.
- Cargo.toml and Cargo.lock changes land in the same commit.

## Releases

`termproof` is published on crates.io (through `0.3.2`); `termproof-cli` and
`termproof-plugin-protocol` are held back (`publish = false`) and are not on
the registry. Releases are cut by `.github/workflows/auto-release.yml`
(weekly, only when something worth releasing landed) and published as GitHub
Releases; the binaries are built by `.github/workflows/release-rust.yml` on
`v*.*.*` tags, and crates.io publishing is
`.github/workflows/publish-crates.yml`, triggered by a published release. The
order, tag format, version-bump rule and pre-release checklist are in
[`docs/publishing.md`](docs/publishing.md). You do not need to know any of
this to contribute a PR — but if you are cutting a release, read it first.

## Licence

By contributing, you agree that your contributions are licensed under the
[MIT Licence](LICENSE), the same terms as the rest of the repository.
