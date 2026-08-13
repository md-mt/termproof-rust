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

The test prints every divergence and two counts, and fails if either drops below
the floor recorded in the test:

| Count | Meaning | At the harness commit | Now |
|---|---|---|---|
| Full agreement | `name`, `passed` and `detail` all match | 26 / 115 | 82 / 115 |
| Verdict agreement | `passed` matches, whatever `detail` says | not recorded | 113 / 115 |
| Panicked | the port took the process down | 5 | 0 |
| Never returned | the port wedged on a deadline it could not reach | 1 | 0 |
| Ran against a real child | the port drove a pseudo-terminal, as the oracle does | 0 / 115 | 28 / 115 |

The two counts are separate floors on purpose. A fix that corrects a verdict and
leaves the wording to a later commit moves the second and not the first, so it
still has to move a number; a wording-only fix moves the first alone.

The panic and never-returned counts are asserted at zero rather than ratcheted.
Recipe-controlled input taking the process down is not a divergence to be traded
off against agreement — see `specs/002-builtin-steps/spec.md` FR-007.

Full agreement is **not** required, because the remaining gap is one open
decision that is not the port's to make plus two rows that belong to another
layer — see "Known residual" below.

## What the corpus does and does not measure

**Does**: the step layer — argument coercion, validation order, timeout
handling, regex dialect, and the exact `detail` string each step produces.

**Does not**: terminal fidelity, for the 87 cases the corpus marks `kind: stub`.
Those run on both sides against a session with fixed content whose wait loops
are transcribed from `termproof/session.py`, so screen rendering, scrollback and
escape-sequence handling stay out of frame for them. The screen layer has its
own work.

**Does, since `PtySession` implements `Session`**: the write path of a real
pseudo-terminal, for the 28 cases the corpus marks `kind: child`. Both halves now
spawn `cat` on a pty and drive it, so `send_text`, `send_line` and `press` are
compared terminal to terminal rather than terminal to double. The count is
ratcheted alongside the agreement floors: routing those cases back to the stub
fails the test.

Three deliberate compromises, each of which inflates or deflates the number in a
knowable direction:

1. **`send_text`, `send_line` and `press` cases run against a real child on both
   sides.** A stub session that appends to a list records `send_text
   {"text": 5}` as *passing*; only a real child reveals the failure. The Python
   half has always used one; the Rust half used its in-memory session until
   `PtySession` implemented `Session`, so a divergence in these rows could
   belong to either the step layer or the session layer. It can no longer: both
   halves spawn a pty child, and a divergence is the port's.
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

### Foreign error text — 30 cases, verdict agrees

A little over a quarter of the corpus embeds an error string owned by CPython or
by libc rather than by TermProof — `could not convert string to float: 'abc'`,
`utf_8_encode() argument 1 must be str, not int`,
`timestamp out of range for platform time_t`,
`unterminated character set at position 0`. Matching them byte-for-byte means
hardcoding a table of another project's messages, keeping it current across
their releases, and inheriting a platform-sensitive one. That is a decision
about what TermProof's diagnostics *are*, not a porting detail, and it is open
as 001-OQ-001 / 002-OQ-002 / 003-OQ-010 — one decision, raised in three specs.

Until it is made, these cases agree on `passed` and diverge on `detail`: the
port reaches the same verdict by the same route and says so in its own words.

### Two `press` rows — verdict differs

`press/ctrl-bracket` (`ctrl-[`) and `press/ctrl-unmapped` (`ctrl-1`) are the only
rows where the two runtimes disagree on `passed`. The oracle accepts both — it
derives the control byte arithmetically — and the port's key table refuses
anything not named in it. That is `termproof-terminal`'s mapping rather than the
step layer's (`specs/002-builtin-steps/spec.md` FR-016), and the shape the port
should adopt is open as OQ-005, because `ctrl-1` produces a byte the oracle
itself would not call meaningful.

Both rows now run against a real pty child on both sides and still diverge, so
the disagreement is the key table's and not an artefact of the port having
replayed them against a double.

### One diagnostic the corpus says is missing

`wait_for_idle` reports `no output observed from the session` when the session
produced nothing at all, and `timed out waiting for idle` otherwise. The port
emits the second for both. It is a real divergence, recorded here rather than
fixed, and the heuristic behind it is OQ-004.
