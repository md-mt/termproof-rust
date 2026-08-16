# termproof

Evidence-first verification for TUI and terminal applications — describe what
a terminal program should do in a recipe, and `termproof` runs it on a real
pseudo-terminal, drives it, checks what it actually did, and leaves the
evidence behind.

[![CI](https://github.com/md-mt/termproof-rust/actions/workflows/rust.yml/badge.svg)](https://github.com/md-mt/termproof-rust/actions/workflows/rust.yml)
[![crates.io](https://img.shields.io/crates/v/termproof)](https://crates.io/crates/termproof)
[![docs.rs](https://docs.rs/termproof/badge.svg)](https://docs.rs/termproof)
[![MIT](https://img.shields.io/github/license/md-mt/termproof-rust)](LICENSE)
[![latest release](https://img.shields.io/github/v/release/md-mt/termproof-rust)](https://github.com/md-mt/termproof-rust/releases)

## Maturity — read this before using it

This repository is an **experimental, in-progress Rust reimplementation** of
[TermProof](https://github.com/md-mt/termproof). The Python implementation at
`md-mt/termproof` is the shipped product and the behavioural oracle; this
port is **not at parity** with it and there is **no parity gate**. `0.x`
releases may change APIs. See [status and parity](docs/status-and-parity.md)
for the measured numbers and the known gaps.

## What TermProof does

TermProof verifies that a terminal or TUI application does what its author
says it does — the way a test suite verifies a library, but against a real
running program on a real pseudo-terminal. You write a **recipe**:

```yaml
recipe_version: 1
name: hello
command:
  argv: [sh, -c, "printf 'hello from termproof\\n'; sleep 2"]
steps:
  - action: wait_for_text
    text: hello from termproof
    timeout_seconds: 5
assertions:
  - type: output_contains
    value: hello from termproof
```

`termproof run` launches the command on a pty, drives the steps against the
live session, evaluates the assertions against what the target actually
printed and how it exited, and writes the evidence: `result.json`,
`report.md`, `raw_output.txt`, `screen.txt`, a `session.cast` recording, and
`latest-report.md` at the output root.

## Installation

**As a library** — the `termproof` crate is published on crates.io:

```sh
cargo add termproof
```

**As a CLI** — `termproof-cli` is deliberately not published to crates.io
(the name is a one-way door and the binary is not ready to spend one), but it
installs from a release tag:

```sh
cargo install --git https://github.com/md-mt/termproof-rust --tag v0.3.2 termproof-cli
```

**Prebuilt binaries** — every release attaches archives for the supported
platforms, each with a `.sha256` checksum and a provenance attestation:

- [`termproof-linux-x86_64.tar.gz`](https://github.com/md-mt/termproof-rust/releases/download/v0.3.2/termproof-linux-x86_64.tar.gz)
- [`termproof-macos-x86_64.tar.gz`](https://github.com/md-mt/termproof-rust/releases/download/v0.3.2/termproof-macos-x86_64.tar.gz)
- [`termproof-macos-arm64.tar.gz`](https://github.com/md-mt/termproof-rust/releases/download/v0.3.2/termproof-macos-arm64.tar.gz)

See [releases](https://github.com/md-mt/termproof-rust/releases) for every tag.

**As a container image** — published to GitHub's registry on every push to
`main` and every release tag, for `linux/amd64`:

```sh
docker run --rm -v "$PWD:/workspace" ghcr.io/md-mt/termproof-rust:latest \
  run hello.recipe.yaml --out /workspace/.termproof/runs
```

Tags are `latest`, the branch name, the release tag, and `sha-<commit>`. The
image carries `rsvg-convert` and `ffmpeg` so the evidence pipeline's rasteriser
and video encoder are present; it does not carry `agg`, and
[`docker/termproof.Dockerfile`](docker/termproof.Dockerfile) says why. Every
pull request builds the image and runs a real recipe inside it before anything
is published — see [`docker/smoke/run-smoke.sh`](docker/smoke/run-smoke.sh),
which you can run yourself:

```sh
docker run --rm --entrypoint /opt/termproof/smoke/run-smoke.sh \
  ghcr.io/md-mt/termproof-rust:latest
```

## Quickstart

From a temporary directory, with a binary installed as above:

```sh
mkdir hello && cd hello
cat > hello.recipe.yaml <<'EOF'
recipe_version: 1
name: hello
command:
  argv: [sh, -c, "printf 'hello from termproof\\n'; sleep 2"]
steps:
  - action: wait_for_text
    text: hello from termproof
    timeout_seconds: 5
assertions:
  - type: output_contains
    value: hello from termproof
EOF
termproof run .
```

The run directory under `.termproof/runs/` contains `result.json`,
`report.md`, `raw_output.txt`, `screen.txt` and `session.cast`, and
`.termproof/runs/latest-report.md` is written at the output root. See
[docs/conditional-recipes.md](docs/conditional-recipes.md) for what the recipe
format deliberately cannot express.

## Supported platforms

| Platform | CI | Release binaries |
|---|---|---|
| Linux x86-64 | tested on every PR (`ubuntu-latest`) | `termproof-linux-x86_64.tar.gz` |
| macOS x86-64 | tested on every PR (`macos-latest`) | `termproof-macos-x86_64.tar.gz` |
| macOS arm64 | binary built on tagged release (`macos-14`) | `termproof-macos-arm64.tar.gz` |
| Windows | **unverified** — no CI job, no binary, not supported yet | — |

A PTY-heavy project gets no Windows badge until real terminal behaviour passes
there; documenting it as tested would be a claim CI does not make.

## Capabilities and status

| Area | Status |
|---|---|
| Recipe model, JSON/YAML loading, Draft 2020-12 schema | **implemented** |
| Seven built-in step actions | **implemented** — measured against the Python oracle: 82/115 full agreement, 113/115 on the pass/fail verdict |
| Eight built-in assertions | **implemented** — measured: 124/147 full agreement, 143/147 on the verdict |
| Real pty execution (`execution: scripted`) | **implemented** — the only execution mode that runs |
| `termproof run` evidence (`result.json`, `report.md`, `raw_output.txt`, `screen.txt`, cast) | **implemented** |
| JUnit XML (`--xml-path`) | **implemented** |
| `--video`, `--diff`, `--update-baselines`, `--skip-unchanged` | **parsed and ignored** — accepted with a warning, not acted on |
| tmux backend, attributed screen, screenshot/video renderers, uploader, `EvidenceCollector` | **library-only** — tested APIs with no CLI caller |
| Branching recipes (`when` predicates) | **not implemented** — deliberately; see [conditional-recipes.md](docs/conditional-recipes.md) |
| Failure containment (structured results for every failure mode) | **not implemented** — see [status and parity](docs/status-and-parity.md) |

The two differential numbers are layer-level measurements, not a product
parity figure; screen fidelity and whole-recipe execution are outside them.
[`docs/status-and-parity.md`](docs/status-and-parity.md) has the full
inventory, the known divergences, and what a run still cannot do.

## Documentation

- [Status and parity](docs/status-and-parity.md) — measured agreement, known
  divergences, library-only surfaces, and what a run cannot do
- [Architecture](docs/architecture.md) — crates, the oracle boundary, the
  session abstractions, and the run pipeline
- [Conditional recipes](docs/conditional-recipes.md) — why the recipe format
  stays linear and what a consumer with a branching scenario uses instead
- [Engineering baseline](docs/engineering-baseline.md) — formatting, lint,
  error, tracing, dependency and feature policy
- [Publishing](docs/publishing.md) — the crates.io publish set, tag format,
  version-bump rule and pre-release checklist
- [Repository governance](docs/governance.md) — GitHub metadata, vulnerability
  settings, merge policy, required checks, the `main` ruleset and audit procedure
- [Reimplementation spec](docs/rust-reimplementation-spec.md) — design
  rationale, compatibility contract and parity gates
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Support](SUPPORT.md)
- [API docs](https://docs.rs/termproof) · [Releases](https://github.com/md-mt/termproof-rust/releases)

## Relationship to `md-mt/termproof`

The two repositories — this one and the
[Python implementation](https://github.com/md-mt/termproof) — were split so
the shipped Python product and this port stop constraining each other's CI,
release surface and packaging. Things that stay with the Python repository on
purpose:

- **The recipe schema and the example corpus.** They are the contract both
  implementations answer to, so they stay with the oracle.
  `load_canonical_schema` in `termproof` therefore finds nothing in this
  checkout.
- **Version and changelog drift checks**, and the sdist packaging gate.

The Python implementation is the only authority on TermProof's behaviour
until a parity gate passes; this port is measured against it, not merged with
it. Branches under `archive/` here hold Rust work that was never merged to
`main` in the original repository, preserved so it is not stranded.

## Layout

- `Cargo.toml` — workspace manifest with shared lint and dependency policy
- `rust-toolchain.toml` — pinned toolchain (`1.96.0`, minimal profile)
- `docs/` — status, architecture, engineering, publishing and spec documents
- `specs/` — the specifications the port is written against, and
  `OBSERVATION-LOG.md`
- `harness/` — the differential harnesses against the Python oracle
- `docker/` — the container image and the recipe its build smoke-tests itself
  with. Not packaged and not shipped by a release, so a change here does not
  cut one (see `.github/scripts/release-decide.py`)
- `crates/` — each crate carries its own `README.md`, which is what crates.io
  renders for it
  - `termproof` — the whole library, and the only crate that publishes
  - `termproof-cli` — binary (`termproof`), command parsing, diagnostics
  - `termproof-plugin-protocol` — versioned process messages, client/host support

## Licence

MIT — see [LICENSE](LICENSE). Same terms as `md-mt/termproof`. Each crate
carries its own copy of the file so it reaches the published tarball; cargo
does not include a workspace-root `LICENSE` in a member's package.
