# termproof-rust

A Rust reimplementation of [TermProof](https://github.com/md-mt/termproof),
extracted from that repository's `rust/` directory with its history intact.

**TermProof is evidence-first verification for TUI and terminal applications.**
The Python implementation at `md-mt/termproof` is the shipped product. This
repository is a port of it, and it is **in progress**.

## Maturity — read this before using it

This port is **not** a drop-in replacement for the Python implementation, and
nothing here should be read as a claim that it behaves the same way.

- There is **no parity gate**. What exists is a differential harness for the
  seven built-in steps: `harness/probe_steps.py` records the Python
  implementation's verdict and diagnostic for each of 115 checked-in cases, and
  `crates/termproof-core/tests/differential_steps.rs` replays the same cases
  through the Rust steps — all seven actions, plus the unknown- and
  missing-action rows, through `steps::dispatch`. It runs in CI as part of
  `cargo test --workspace`, and it asserts a floor rather than equality. On that
  corpus the two runtimes reach **82 of 115** in full agreement — name, verdict
  and diagnostic text — and **113 of 115** on the pass/fail verdict alone, up
  from 26 of 115 when the harness first ran; panics and cases that never return
  are asserted at zero rather than ratcheted. `harness/README.md` is the
  authority on those counts and on where the remaining 33 cases diverge.
- **That is a step-layer number, not a product number.** The corpus drives the
  seven steps against a session with fixed content. Whole-recipe execution, the
  assertion layer and terminal fidelity — rendering, scrollback, escape
  sequences — are outside it, and no equivalent measurement exists for them.
  Reading 82 of 115 as a parity figure for the port would be wrong.
- Known defects and divergences, in rough severity order: the in-memory session
  double `InMemorySession` encodes test-passing rather than PTY semantics —
  `wait_for_text` answers from fixed content and ignores its deadline, and
  `wait_for_idle` always returns true — so nothing running through it can show a
  deadline being honoured; two `press` rows (`ctrl-[`, `ctrl-1`) where the
  port's key table refuses a key the oracle accepts, which is
  `termproof-terminal`'s mapping to settle; `wait_for_idle` does not distinguish
  "no output observed from the session" from an ordinary idle timeout; and 30
  cases where the port reaches the oracle's verdict but words the diagnostic
  itself, because the oracle's wording there is CPython's or libc's and whether
  to copy it is an open decision. The five defects this section listed before —
  a large finite `timeout_seconds` / `seconds` panicking the process, valid
  Python regexes rejected, strict JSON typing rejecting recipes Python accepts,
  `send_line` silently discarding a non-string `text`, and match diagnostics
  diverging from Python's `repr` — are fixed, each with a test that failed
  first.
- The PTY backend and terminal screen in `termproof-terminal` are no longer
  stubs: children run on a real pseudo-terminal via `portable-pty`, and the
  screen is a `vt100` cell grid that interprets escapes instead of stripping
  them. **Nothing consumes them yet.** `PtySession` implements no `Session`,
  the only `Session` implementations are `InMemorySession` and
  `StubDockerSession`, and `termproof run` prints a summary of its arguments
  rather than executing recipes. So the terminal layer is correct but not yet
  reachable from the CLI, and the step-layer numbers above do not exercise it.

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
- `specs/` — the specifications the port is written against, and
  `OBSERVATION-LOG.md`, which records what was measured and what was left out
- `harness/` — the differential step harness: the Python probe, the checked-in
  corpus and the recorded oracle expectations
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
