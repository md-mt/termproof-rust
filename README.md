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
  `crates/termproof/tests/differential_steps.rs` replays the same cases
  through the Rust steps — all seven actions, plus the unknown- and
  missing-action rows, through `steps::dispatch`. It runs in CI as part of
  `cargo test --workspace`, and it asserts a floor rather than equality. On that
  corpus the two runtimes reach **82 of 115** in full agreement — name, verdict
  and diagnostic text — and **113 of 115** on the pass/fail verdict alone, up
  from 26 of 115 when the harness first ran; panics and cases that never return
  are asserted at zero rather than ratcheted. `harness/README.md` is the
  authority on those counts and on where the remaining 33 cases diverge.
- **That is a step-layer number, not a product number.** 87 of the 115 cases
  drive the steps against a session with fixed content; the other 28 — every
  `send_text`, `send_line` and `press` row — now spawn a real pseudo-terminal
  child on both sides, so the pty write path is in frame where it was not
  before. Screen fidelity — rendering, scrollback, escape sequences — is still
  outside it, as is whole-recipe execution, and no equivalent measurement exists
  for either. The assertion layer has a measurement of its own, below; the step
  and assertion corpora are the only two that exist. Reading 82 of 115 as a
  parity figure for the port would be wrong.
- Known defects and divergences, in rough severity order: the in-memory session
  double `InMemorySession` encodes test-passing rather than PTY semantics —
  `wait_for_text` answers from fixed content and ignores its deadline, and
  `wait_for_idle` always returns true — so nothing running through it can show a
  deadline being honoured; two `press` rows (`ctrl-[`, `ctrl-1`) where the
  port's key table refuses a key the oracle accepts, which is the `terminal`
  module's mapping to settle; `wait_for_idle` does not distinguish
  "no output observed from the session" from an ordinary idle timeout; and 30
  cases where the port reaches the oracle's verdict but words the diagnostic
  itself, because the oracle's wording there is CPython's or libc's and whether
  to copy it is an open decision. The five defects this section listed before —
  a large finite `timeout_seconds` / `seconds` panicking the process, valid
  Python regexes rejected, strict JSON typing rejecting recipes Python accepts,
  `send_line` silently discarding a non-string `text`, and match diagnostics
  diverging from Python's `repr` — are fixed, each with a test that failed
  first.
- The PTY backend and terminal screen in `termproof::terminal` are no longer
  stubs, and they are now reachable: children run on a real pseudo-terminal via
  `portable-pty`, the screen is a `vt100` cell grid that interprets escapes
  instead of stripping them, `PtySession` implements `Session`, and
  `PtySessionBackend` is what a run uses by default. `termproof run` executes
  recipes rather than printing a summary of its arguments — it discovers recipe
  files under the paths given, loads each one, plans recipe × renderer, runs the
  steps against a real child, and writes `result.json`, `report.md`,
  `raw_output.txt`, `screen.txt` and the asciicast per run, plus
  `latest-report.md` and a JUnit file when `--xml-path` is given.
- The eight built-in assertions are implemented and evaluated by a real run, so
  a recipe that declares an assertion or an expected exit code now reports a
  verdict about what the target did. They are measured the same way the steps
  are, against a 165-case corpus recorded from the Python implementation:
  **124/147 full agreement, 143/147 verdict agreement**, and all 18 inputs that
  end the Python run contained rather than losing the report. The 23 remaining
  divergences are enumerated in `harness/README.md` — four are the only rows
  where the two runtimes disagree on pass/fail, and they come from Python's JSON
  decoder accepting `NaN` and `Infinity`.
- **There is new library surface that no command reaches yet.** Eight modules
  landed as APIs with tests and no caller, so they are worth knowing about and
  worth not mistaking for features:
  - `termproof::terminal` grew `attributed`, a per-cell screen carrying
    foreground, background, bold, dim, italic, underline, strikethrough,
    reverse and display width, with an SVG renderer; `tmux`, a `Session` that
    runs the program in a tmux pane and reads the grid back with
    `capture-pane`, so a disagreement between it and the `vt100` path is an
    emulation gap made visible; and `proc`, child processes with a deadline.
  - `termproof::terminal` also grew `driver`, a scenario-facing wrapper over
    `Box<dyn Session>`. There are now two ways to talk to a session and they
    point in opposite directions: **implement `Session` to write a backend,
    use `SessionDriver` to write a scenario.** The trait stays narrow and
    totally fallible because that is what makes a backend cheap to write; the
    driver supplies default timeouts, a `screen_contains` convenience, and
    defers errors so a failed keystroke is reported once at the assertion —
    naming the call that first failed — instead of at every `?`.
  - `termproof::evidence` grew `screenshot` and `cast_video`, which render
    stills and video frames through that one renderer rather than two unrelated
    ones; `dedup`, which skips re-rendering a screen identical to the step
    before it; and `uploader`, a publishing seam with a fallback chain that
    records which store it fell back from.
  - The `termproof` crate root grew `parity`, which compares two runs and
    reports where they disagree; `before_after`, which reports which outcomes
    flipped; `selection`, which maps a changeset onto recipes via `ci_paths`;
    `run_config`, a whole run described by one file; and `vocabulary`, a
    configurable failure detector.

  **None of this is wired into `termproof run`.** The screenshots a run writes
  today are still the single-colour text path, no run uses the tmux backend or
  the deduper, and nothing calls the uploader. Treat these as a library a
  caller could build on, not as behaviour the CLI has.
- **What a run still cannot do**, so that a green exit is not read as more than
  it is:
  - **Only `execution: scripted` on a pty runs.** A recipe whose
    `command.pty` is false, or whose `execution` is anything else, is refused
    with a diagnostic naming the reason and a non-zero exit. Running it on a
    pty anyway would report a verdict about something the recipe did not ask
    for. `StubDockerSession` is still a stub.
  - **`--video`, `--diff`, `--update-baselines` and `--skip-unchanged` are
    parsed and ignored**, with a warning on stderr rather than silence.
  - **A recipe cannot branch on what it observes**, and that is a decision
    rather than a gap. Polling until something renders and acting only if it
    did, dismissing an overlay that may or may not appear, and retrying a racy
    step all belong in a consumer's own runner, written against `SessionDriver`
    above. What the crate offers such a consumer instead — and why a `when`
    predicate and a second, imperative recipe model were both declined — is
    [`docs/conditional-recipes.md`](docs/conditional-recipes.md).
  - **Failures are not contained.** RUST-009 (issue #2) — turning recipe, step,
    plugin, process and PTY failures into structured results — is untouched
    here.

Until a parity gate passes, treat the Python implementation as the only
authority on TermProof's behaviour.

## Layout

- `Cargo.toml` — workspace manifest with shared lint and dependency policy
- `rust-toolchain.toml` — pinned toolchain (`1.96.0`, minimal profile)
- `docs/engineering-baseline.md` — formatting, lint, error, tracing, dependency,
  feature and unsafe-code policy
- `docs/publishing.md` — the verified crates.io publish order, the version-bump
  rule, the pre-release checklist and what the tarballs deliberately leave out
- `docs/conditional-recipes.md` — why the recipe format stays linear, what a
  consumer with a branching scenario uses instead, and what would reopen it
- `docs/rust-reimplementation-spec.md` — the design rationale, compatibility
  contract and parity gates this port is measured against. Written before the
  split, so parts of it describe a workspace under `rust/` in the Python
  repository; its header says which sections are superseded.
- `specs/` — the specifications the port is written against, and
  `OBSERVATION-LOG.md`, which records what was measured and what was left out
- `harness/` — the differential step harness: the Python probe, the checked-in
  corpus and the recorded oracle expectations
- `crates/` — each crate carries its own `README.md`, which is what crates.io
  renders for it
  - `termproof` — the whole library, and the only crate that publishes. Its
    root is models, config, schema, registries, planning and orchestration;
    `terminal` is PTY/tmux/process sessions, terminal screen and cast
    recording; `evidence` is rendering, reports, video, baselines, diff, dedup
    and publishing. It was merged from `termproof-core`, `termproof-terminal`
    and `termproof-evidence` before any of them was published — see that
    crate's README for why the layout is this one.
  - `termproof-cli` — binary (`termproof`), command parsing, diagnostics
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
  in `termproof` therefore finds nothing in this checkout.
- **Version and changelog drift checks**, and the sdist packaging gate.

Branches under `archive/` here hold Rust work that was never merged to `main` in
the original repository, preserved so it is not stranded — including the fuller
quality-gate tooling (`deny.toml`, a coverage baseline, a conformance corpus and
their checker scripts) that only ever existed on `wt/rust-003-ci-gates`.

## Publishing

Nothing has been published to crates.io yet. One crate is in scope when it
happens — `termproof`. `termproof-cli` and `termproof-plugin-protocol` carry
`publish = false` on purpose; a crates.io name is a one-way door, and those two
are not ready to spend one.

Releases go out through `.github/workflows/publish-crates.yml`, triggered by
publishing a GitHub release. It derives the publish set and order from
`cargo metadata` rather than from a list anyone has to maintain, refuses a tag
that disagrees with the workspace version, gates on the full test suite, and is
safe to re-run after a partial failure. The order, the tag format, the
version-bump rule and the pre-release checklist are in
[`docs/publishing.md`](docs/publishing.md).

## Licence

MIT — see [LICENSE](LICENSE). Same terms as `md-mt/termproof`. Each crate
carries its own copy of the file so it reaches the published tarball; cargo
does not include a workspace-root `LICENSE` in a member's package.
