# Status and parity

This page is the deep version of the README's maturity notice. It is written
to be read **before** you depend on anything in this repository, and it is
kept deliberately honest: what the port measures, what it does not, and where
it is known to diverge from the Python implementation. The
[`harness/README.md`](../harness/README.md) is the authority on every number
here — this page summarises it, it does not replace it.

## What this port is

`termproof-rust` is a Rust reimplementation of
[TermProof](https://github.com/md-mt/termproof). The Python implementation at
`md-mt/termproof` is the **shipped product and the behavioural oracle**. This
port is **in progress** and is **not a drop-in replacement**: there is **no
parity gate**, and nothing here should be read as a claim that the port
behaves the same way as the Python implementation.

Until a parity gate passes, treat the Python implementation as the only
authority on TermProof's behaviour.

Pre-1.0 `0.x` releases may change public APIs without notice. A breaking
change is a minor bump (`0.2.x` → `0.3.0`) under the workspace's
[version-bump rule](publishing.md#version-bump-rule), but that is a semver
convention, not a stability promise: the port's public surface is still
moving as it approaches parity.

## The differential harness

The step and assertion layers are measured against corpora recorded from the
Python implementation. Each layer has its own harness, each the same two-half
shape: a Python probe drives the oracle over a checked-in corpus, and a Rust
test replays the same cases through the port and reports agreement.

| Layer | Oracle | Port | Corpus |
|---|---|---|---|
| Steps | `harness/probe_steps.py` | `crates/termproof/tests/differential_steps.rs` | `harness/corpus/cases.json` |
| Assertions | `harness/probe_assertions.py` | `crates/termproof/tests/differential_assertions.rs` | `harness/corpus/assertion_cases.json` |

The harnesses assert a **floor rather than equality**: agreement can rise but
must never fall, and panics and cases that never return are asserted at zero
rather than ratcheted. Full agreement is deliberately **not** required — the
remaining gaps are open decisions that are not the port's to make (see
[Known defects and divergences](#known-defects-and-divergences) below).

## Step layer

| Count | Meaning | Now |
|---|---|---|
| Full agreement | `name`, `passed` and `detail` all match | **82 / 115** |
| Verdict agreement | `passed` matches, whatever `detail` says | **113 / 115** |
| Panicked | the port took the process down | 0 |
| Never returned | the port wedged on a deadline it could not reach | 0 |
| Ran against a real child | the port drove a pseudo-terminal, as the oracle does | 28 / 115 |

Up from 26 of 115 when the harness first ran. The remaining 33 cases are
enumerated in `harness/README.md`.

## Assertion layer

| Count | Meaning | Now |
|---|---|---|
| Full agreement | `name`, `passed` and `detail` all match | **124 / 147** |
| Verdict agreement | `passed` matches, whatever `detail` says | **143 / 147** |
| Contained | the oracle ends its run; the port returns a result instead | 18 / 18 |
| Escaped containment | the port also lost results | 0 |
| Panicked | the port took the process down | 0 |
| Never returned | the port wedged | 0 |

The denominator is 147, not 165: the eighteen contained cases have no oracle
verdict to agree with. With `termproof`'s `json-schema` feature off, the 58
`json_schema` cases are skipped by assertion type and the rest is 89 / 89 full
and verdict agreement — every one of the 23 default-run detail divergences and
all 4 verdict divergences is a `json_schema` case.

## What the numbers do not cover

These are layer-level numbers, not a product number:

- **Screen fidelity is outside them.** 87 of the 115 step cases drive the
  steps against a session with fixed content; rendering, scrollback and
  escape-sequence handling stay out of frame. The assertion corpus fixes
  `screen`, `raw_output` and `exit_code` as strings, so terminal fidelity,
  the PTY and real exit-code capture are out of frame there too.
- **Whole-recipe execution is outside them.** The eleven assertion `run`
  cases stop at the evaluated list, the score and the overall verdict; they
  do not exercise the planner, the runner or the evidence pipeline as a run
  would.
- **The assertion layer can only be as right as what it is handed.** An
  assertion is evaluated against fixed strings, so a divergence upstream of
  it cannot be detected here.
- **Non-POSIX path semantics are not handled.** `crates/termproof/src/pypath.rs`
  models `PurePosixPath`; Windows drive letters and UNC paths are not in the
  corpus and are not supported.

Reading 82/115 or 124/147 as a parity figure for the port would be wrong.

## Known defects and divergences

In rough severity order:

- **The in-memory session double encodes test-passing rather than PTY
  semantics.** `InMemorySession`'s `wait_for_text` answers from fixed content
  and ignores its deadline, and `wait_for_idle` always returns true — so
  nothing running through it can show a deadline being honoured.
- **Two `press` rows diverge on verdict.** `press/ctrl-bracket` (`ctrl-[`)
  and `press/ctrl-unmapped` (`ctrl-1`) are the only step rows where the two
  runtimes disagree on `passed`. The oracle derives the control byte
  arithmetically; the port's key table refuses anything not named in it.
  This is the `terminal` module's mapping to settle (`specs/002-builtin-steps/spec.md`
  FR-016, OQ-005).
- **`wait_for_idle` does not distinguish** "no output observed from the
  session" from an ordinary idle timeout (OQ-004).
- **Foreign error text — 30 step cases and 15 assertion cases, verdict
  agrees.** A little over a quarter of the step corpus embeds an error string
  owned by CPython or libc (`could not convert string to float: 'abc'`,
  `utf_8_encode() argument 1 must be str, not int`, …). The port reaches the
  same verdict by the same route and words the diagnostic itself; matching
  byte-for-byte means hardcoding another project's messages and keeping them
  current across releases. That is an open decision about what TermProof's
  diagnostics *are* (001-OQ-001 / 002-OQ-002 / 003-OQ-010), not a porting
  detail.
- **Non-finite JSON — 4 assertion cases, verdict differs.** Python's `json`
  decoder accepts bare `NaN`, `Infinity` and `-Infinity`; `serde_json`
  rejects them. These are the only four rows where the two runtimes disagree
  on `passed` (003-OQ-008).
- **Object key order — 1 assertion case.** Python dicts preserve insertion
  order; `serde_json::Map` without `preserve_order` is a `BTreeMap`. Turning
  the feature on would fix this row and change how every other JSON object in
  the crate is ordered, a workspace-wide decision.
- **`best_match` tie-breaks — 2 assertion cases.** When two root-level errors
  have identical relevance keys, the winner is decided by the validator's
  yield order, which the port cannot see through its map type.

Five earlier defects are fixed, each with a test that failed first: a large
finite `timeout_seconds` / `seconds` panicking the process, valid Python
regexes rejected, strict JSON typing rejecting recipes Python accepts,
`send_line` silently discarding a non-string `text`, and match diagnostics
diverging from Python's `repr`.

## Library surface no TermProof command uses yet

Fourteen modules landed as tested APIs with no caller in the CLI. They are
worth knowing about and worth **not** mistaking for features of `termproof
run`:

- `termproof::parity` — compares two runs and reports where they disagree.
- `termproof::before_after` — reports which outcomes flipped.
- `termproof::selection` — maps a changeset onto recipes via `ci_paths`.
- `termproof::run_config` — a whole run described by one file.
- `termproof::vocabulary` — a configurable failure detector.
- `termproof::build_info` — provenance for the binary under test.
- `terminal::attributed` — a per-cell screen carrying colours, styles and
  display width, with an SVG renderer.
- `terminal::tmux` — a `Session` that runs the program in a tmux pane and
  reads the grid back with `capture-pane`.
- `terminal::proc` — child processes with a deadline.
- `terminal::driver` — `SessionDriver`, a scenario-facing wrapper over
  `Box<dyn Session>`.
- `evidence::screenshot` and `evidence::cast_video` — stills and video frames
  through one renderer.
- `evidence::dedup` — skips re-rendering a screen identical to the step
  before it.
- `evidence::uploader` — a publishing seam with a fallback chain.
- `evidence::collector` — `EvidenceCollector`, the ordered step model those
  plug into; `publish` renders, dedupes, uploads and writes an
  `evidence.json` manifest in one pass. It sits **beside** `RunResult` rather
  than inside it.

**None of this is wired into `termproof run`.** A run writes
`raw_output.txt`, `screen.txt` and a cast if one was recorded — no image at
all, so no command reaches either renderer. No run uses the tmux backend or
the deduper, and nothing calls the uploader. Treat these as a library a
caller could build on, not as behaviour the CLI has.

## What a run still cannot do

So that a green exit is not read as more than it is:

- **Only `execution: scripted` on a pty runs.** A recipe whose `command.pty`
  is false, or whose `execution` is anything else, is refused with a
  diagnostic naming the reason and a non-zero exit. `DockerSessionBackend` is
  still a stub.
- **`--video`, `--diff`, `--update-baselines` and `--skip-unchanged` are
  parsed and ignored**, with a warning on stderr rather than silence.
- **A recipe cannot branch on what it observes** — and that is a decision
  rather than a gap. Polling until something renders and acting only if it
  did, dismissing an overlay that may or may not appear, and retrying a racy
  step all belong in a consumer's own runner, written against `SessionDriver`.
  What the crate offers such a consumer instead — and why a `when` predicate
  and a second, imperative recipe model were both declined — is
  [`conditional-recipes.md`](conditional-recipes.md).
- **Failures are not contained.** RUST-009 (issue #2) — turning recipe, step,
  plugin, process and PTY failures into structured results — is untouched.
