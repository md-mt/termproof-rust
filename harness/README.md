# Differential harness — step semantics

A cross-runtime differential harness for the seven built-in steps. It exists
because the port claimed corpus parity several times over with green local
gates, and a differential run against the Python implementation still found the
two runtimes agreeing on a minority of cases. A number nobody can reproduce is
not a measurement.

## Shape

The harness is two halves that meet at a checked-in corpus.

| Half | Where | What it does |
|---|---|---|
| Oracle | `probe_steps.py` | Drives the Python steps over `corpus/cases.json` and records each case's `name`, `passed` and `detail` into `corpus/steps.expected.json`. |
| Port | `crates/termproof-core/tests/differential_steps.rs` | Replays the same cases through the Rust steps and reports the agreement count. |

Splitting it this way means the measurement is reproducible in CI without a
Python interpreter, and the recorded expectations carry the environment they
were observed in — several details are CPython-, libc- and `ptyprocess`-version
dependent (`specs/002-builtin-steps/spec.md` FR-004, FR-008, FR-016).

## Regenerating the expectations

```sh
cd /path/to/python/termproof
TERMPROOF_PYTHON_REPO=$PWD uv run python \
    /path/to/termproof-rust/harness/probe_steps.py \
    > /path/to/termproof-rust/harness/corpus/steps.expected.json
```

Only regenerate deliberately: the file is the oracle's testimony, and quietly
re-recording it turns a failing comparison into a passing one without changing
any behaviour.

## Reading the number

```sh
cargo test -p termproof-core --test differential_steps -- --nocapture
```

The test prints every divergence and the agreement count, and fails if the count
drops below the floor recorded in the test. It does **not** require full
agreement, because a chunk of the corpus is currently blocked on a decision that
is not the port's to make — see "Known residual" below.

## What the corpus does and does not measure

**Does**: the step layer — argument coercion, validation order, timeout
handling, regex dialect, and the exact `detail` string each step produces.

**Does not**: terminal fidelity. Both halves run against a session with fixed
content whose wait loops are transcribed from `termproof/session.py`, so screen
rendering, scrollback and escape-sequence handling are out of frame. The PTY and
screen layers have their own work.

Three deliberate compromises, each of which inflates or deflates the number in a
knowable direction:

1. **`send_text`, `send_line` and `press` cases run against a real `pexpect`
   child on the Python side.** A stub session that appends to a list records
   `send_text {"text": 5}` as *passing*; only a real child reveals the failure.
   The Rust side runs them against its in-memory session, so divergences in
   these rows can belong to either the step layer or the session layer, and the
   test's report attributes them.
2. **`NaN`, `Infinity` and `-Infinity` cannot be written as JSON numbers.** The
   corpus spells them `"@nan"`, `"@inf"` and `"@-inf"`. Python's `json` module
   accepts bare `NaN` tokens and Rust's does not, so each half substitutes the
   spelling its own parser accepts and that its duration coercion maps to the
   same float. The step under test sees the same `f64` either way.
3. **A step object with no `action` is absent from the corpus.** It kills the
   Python run outright — the runner's own exception handler reads
   `step["action"]` as its first line — so there is no oracle verdict to record.
   `specs/002-builtin-steps/spec.md` FR-025 supersedes the oracle here and
   OQ-008 leaves the replacement diagnostic undecided.

## Known residual

A little over a fifth of the corpus embeds an error string owned by CPython or
by libc rather than by TermProof — `could not convert string to float: 'abc'`,
`utf_8_encode() argument 1 must be str, not int`,
`timestamp out of range for platform time_t`,
`unterminated character set at position 0`. Matching them byte-for-byte means
hardcoding a table of another project's messages, keeping it current across
their releases, and inheriting a platform-sensitive one. That is a decision
about what TermProof's diagnostics *are*, not a porting detail, and it is open
as 001-OQ-001 / 002-OQ-002 / 003-OQ-010 — one decision, raised in three specs.

Until it is made, these cases agree on `passed` and diverge on `detail`. The
test reports the two counts separately so the residual is visible rather than
buried.
