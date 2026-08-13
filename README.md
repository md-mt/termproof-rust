# termproof-rust

A Rust reimplementation of [TermProof](https://github.com/md-mt/termproof),
extracted from that repository's `rust/` directory with its history intact.

**TermProof is evidence-first verification for TUI and terminal applications.**
The Python implementation at `md-mt/termproof` is the shipped product. This
repository is a port of it, and it is **in progress**.

## Maturity — read this before using it

This port is **not** a drop-in replacement for the Python implementation, and
nothing here should be read as a claim that it behaves the same way.

- There is **no parity gate**. A differential harness driving identical inputs
  through both implementations found them agreeing on **55 of 217 cases**, with
  33 pass/fail flips and 6 panics on recipe-controlled input. Landing that
  harness as a required check is the next piece of work, and it will be red when
  it arrives.
- Known defects carried over with the code, in rough severity order: a large
  finite `timeout_seconds` / `seconds` value panics the process; valid Python
  regexes are rejected; strict JSON typing rejects recipes Python accepts;
  `send_line` silently discards a non-string `text`; `_format_match`
  diagnostics do not match Python's; `MockSession` encodes test-passing rather
  than PTY semantics; the step dispatch table is exercised for 1 of 7 actions
  and `wait_for_idle` has no coverage.
- The PTY backend and terminal screen in `termproof-terminal` are no longer
  stubs: children run on a real pseudo-terminal via `portable-pty`, and the
  screen is a `vt100` cell grid that interprets escapes instead of stripping
  them. **Nothing consumes them yet.** `PtySession` implements no `Session`,
  the only `Session` implementations are `InMemorySession` and
  `StubDockerSession`, and `termproof run` does not execute recipes. So the
  terminal layer is correct but not yet reachable from the CLI, and the parity
  numbers above are unchanged by it.

Until a parity gate passes, treat the Python implementation as the only
authority on TermProof's behaviour.

## Layout

- `Cargo.toml` — workspace manifest with shared lint and dependency policy
- `rust-toolchain.toml` — pinned toolchain (`1.96.0`, minimal profile)
- `docs/engineering-baseline.md` — formatting, lint, error, tracing, dependency,
  feature and unsafe-code policy
- `docs/rust-reimplementation-spec.md` — the design rationale, compatibility
  contract and parity gates this port is measured against. Written before the
  split, so parts of it describe a workspace under `rust/` in the Python
  repository; its header says which sections are superseded.
- `crates/`
  - `termproof-cli` — binary (`termproof`), command parsing, diagnostics
  - `termproof-core` — models, config, schema, registries, planning, orchestration
  - `termproof-terminal` — PTY/process sessions, terminal screen, cast recording
  - `termproof-evidence` — rendering, reports, video, baselines, diff, cache
  - `termproof-plugin-protocol` — versioned process messages, client/host support

## Quickstart

```sh
cargo run -p termproof-cli

# The gates CI enforces
cargo fmt --check --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## Relationship to `md-mt/termproof`

The two repositories were split so the shipped Python product and this port stop
constraining each other's CI, release surface and packaging. Things that stay
with the Python repository on purpose:

- **The recipe schema and the example corpus.** They are the contract both
  implementations answer to, so they stay with the oracle. `load_canonical_schema`
  in `termproof-core` therefore finds nothing in this checkout.
- **Version and changelog drift checks**, and the sdist packaging gate.

Branches under `archive/` here hold Rust work that was never merged to `main` in
the original repository, preserved so it is not stranded — including the fuller
quality-gate tooling (`deny.toml`, a coverage baseline, a conformance corpus and
their checker scripts) that only ever existed on `wt/rust-003-ci-gates`.

## Licence

MIT — see [LICENSE](LICENSE). Same terms as `md-mt/termproof`.
