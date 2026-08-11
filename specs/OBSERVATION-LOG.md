# Observation log — first spec set

**Created**: 2026-08-11

Constitution §"Specification Discipline" requires that observations which did *not* become
requirements are recorded, so the omissions are auditable as choices rather than gaps. This
file records how the first spec set was derived, what was measured, what was deliberately left
out, and the consolidated list of open questions.

Specs covered: `001-recipe-format`, `002-builtin-steps`, `003-builtin-assertions`.

---

## 1. The oracle environment

Every `OBSERVED` claim in the three specs was measured against:

| | |
|---|---|
| Oracle source | `md-mt/termproof` (Python), read-only, at the revision this port targets |
| Interpreter | CPython 3.12 |
| `pexpect` | 4.9.0 |
| `ptyprocess` | 0.7.0 |
| `jsonschema` | 4.26.0 |
| `pyte` | 0.8.2 |
| Platform | macOS (darwin 25.6.0), arm64 |

**None of these are pinned by the oracle.** `pyproject.toml` declares floors
(`jsonschema>=4.0`, `pexpect>=4.9.0`, `pyte>=0.8.2`), so a fresh `pip install` can resolve
differently and change observed behaviour. Two requirements are known to be
resolution-sensitive: 002-FR-016 (`ctrl-<unmapped>`, `ptyprocess`) and 003-FR-016/FR-017
(`jsonschema` messages and `best_match`). Pinning is proposed in 002-OQ-005 and 003-OQ-010.

The oracle repository ships no virtualenv, and the sandbox blocks writes to it. A separate
environment was built outside the repo and the oracle was imported by path. **No file in the
Python repository was modified.**

---

## 2. Where each spec's requirements came from

Counting `Authority:` lines by their highest-ranked rung (constitution Principle IV):

| Spec | `SCHEMA`/`DOC` | `OBSERVED` | `SOURCE` only | Constitution |
|---|---|---|---|---|
| 001 recipe format | 6 | 14 | 5 | 2 |
| 002 built-in steps | 0 | 22 | 3 | 3 |
| 003 built-in assertions | 0 | 17 | 5 | 3 |

The `SOURCE`-only requirements are the ones to distrust, and each says so in place. They are:

- **001-FR-005** (`source_path`) — internal field, invisible from outside. Raised as OQ-008.
- **001-FR-007** (`recipe_version` rejects `true`) — a deliberate `isinstance(..., bool)`
  carve-out; the code tests for it specifically, which is why it is treated as a decision
  rather than an accident.
- **001-FR-009** (the loader coercion table) — the individual rows were observed; the claim
  that the table is *exhaustive* comes from reading `recipe_from_mapping`.
- **001-FR-014**, **001-FR-015**, **001-FR-016**, **001-FR-023**, **001-FR-025** — the issue
  data structure, path rendering rules, missing-property anchoring, error suppression, and the
  closedness of the legacy list. Individual outputs were observed; the *rules* behind them are
  source.
- **002-FR-003**, **002-FR-026**, **002-FR-027** — screen capture, ordering, registry dispatch.
- **003-FR-014** (`file_contains` ignores `detail`), **003-FR-022** (scoring),
  **003-FR-023** (overall `passed`), **003-FR-024** (registry), **003-FR-017**(4)
  (`best_match`).

Nothing in the spec set rests on `DOC` alone; where `docs/recipe-format-v1.md` states a
default, the loader was run to confirm it.

---

## 3. The probes

Three harnesses, kept outside the repository as scratch. Each is described here in enough
detail to rebuild, because the conformance harness (issue #3) should supersede them.

### 3.1 Loader and validator probe

Called `recipe_from_mapping`, `validate_recipe_mapping` and `validate_recipe_file` directly
over ~40 documents. No fakes involved: these functions are pure.

Covered: every default in 001-FR-004; `recipe_version` at `1`/`2`/`"1"`/`true`/absent;
coercion of `timeout_seconds` and `cols`; `renderers` at `5`/`null`; the `cols`/`rows`/
`expect_exit_code` integer overrides; unknown step and assertion names; every row of the
legacy-tolerance table; the four `$`-level document failures; path rendering at
`command.argv[0]` and `steps[0].timeout_seconds`.

### 3.2 Step probe

Drove the seven step classes over 137 cases through a **stub session**, wrapped in a
transcription of the runner's `try/except`, with a 2-second `SIGALRM` per case so a hanging
case reported `BLOCKS` instead of stalling the probe.

**The stub is the weak point of this whole exercise, and it is the exact mistake constitution
Principle IV was written about.** The stub's `wait_for_text` and `wait_for_idle` bodies were
transcribed from `session.py`, so an error in the transcription would produce a wrong
observation that looks authoritative. Worse, a stub whose `send_text` appends to a list
records `send_text {text: 5}` as **passing**, because it never encodes the value — which is
precisely what both the port's harness and the adversarial review's harness did, and why the
review's report contains that wrong claim.

Rows the stub cannot be trusted for were re-measured against a real child (§3.3). The
conformance harness must not use a stub session at all — 002-SC-001 requires a real child
process.

### 3.3 Real-child probe

Spawned an actual `pexpect` child and exercised the paths a stub cannot reach.

**Two claims in the adversarial review were corrected by this probe:**

1. The review records `press ctrl-1` as failing in Python. Under `ptyprocess` 0.7.0,
   `sendcontrol` returns `(0, b'')` for an unmapped character — it does not raise. The step
   **passes and sends nothing**. Recorded as 002-FR-016 with the version caveat, and raised as
   002-OQ-005.
2. The review records `send_text {text: 5}` as passing in Python. Against a real child,
   `send(5)` raises `TypeError: utf_8_encode() argument 1 must be str, not int`. Recorded as
   002-FR-022.

Also established here: the `ptyprocess` control-character table (read from its source and
confirmed by execution); that `sendcontrol` lowercases a *second* time, so `ctrl-A` ≡
`ctrl-a`; that `'ENTERİ'.lower()` is `'enteri̇'`, confirming Unicode folding (002-OQ-006); and
that `wait_for_idle`'s `no output observed from the session` branch requires a **live** silent
child, because a dead session short-circuits the step to success (003 — no, 002-FR-011, and
002-OQ-004).

### 3.4 Assertion probe

Drove the eight assertion classes over 78 cases against real temporary files, wrapped in a
transcription of the runner's dispatch — including its **absence** of a handler, so an
exception was recorded as `RUN_ABORTS` rather than as a result. Assertions are pure with
respect to the session, so no child was needed. The ordering behaviour of
`evaluate_assertions` (003-FR-019) was reproduced from source and checked against three
recipe shapes.

---

## 4. Observations deliberately left out of the specs

Recorded so the omissions are auditable.

- **Absolute paths in `file_exists` and `json_schema` details.** The observed details embed
  the temporary fixture directory. Only the *shape* is specified (003-FR-012, 003-FR-021); the
  conformance harness must substitute the fixture root before diffing.
- **Wall-clock timing.** Several steps' pass/fail depends on how fast the polling loop runs
  relative to the stable window. No requirement mentions elapsed time, and the corpus must not
  encode any, or the gate will be flaky.
- **`pyte` screen fidelity.** `screen` contents — wrapping, scrollback, escape-sequence
  coverage — were not probed. They belong to the session layer, which is out of scope.
- **`float()` accepting underscores and unusual literals.** `float("1_0")` is `10.0` in Python
  and `float("infinity")` is `inf`. Observed in passing, not specified: no plausible recipe
  relies on it, and specifying it would freeze more CPython vocabulary (002-OQ-002).
- **`json.loads` accepting `-Infinity` nested inside structures.** Confirmed, folded into the
  general statement in 003-FR-018 rather than enumerated.
- **The `checks`, `ci_paths` and `operator` fields.** They load and default correctly
  (001-FR-004) but nothing in the first spec set consumes them. Their semantics belong to the
  reporter and evidence specs.
- **The exact `errno` text in `schema file unreadable`.** `[Errno 2] No such file or
  directory` was observed; `[Errno 13] Permission denied` and `[Errno 21] Is a directory` were
  not. They are the same class of foreign string as 003-OQ-010 and inherit its decision.
- **Windows behaviour.** Nothing was measured off macOS. `002-FR-008`'s `time_t` message and
  the PTY layer generally are platform-sensitive.

---

## 5. Consolidated open questions

There are **22**, across three specs. Per constitution §"Specification Discipline", finding
these is a success: each one is a place where the Python implementation is ambiguous,
underspecified, or accidental, and freezing a guess is how a rewrite inherits bugs. **None was
resolved by picking whatever the current implementation happens to do.**

Five of them (marked ★) block work that is already in flight, because the Rust implementation
must do *something* and the oracle does not say what.

### One decision that appears three times

**001-OQ-001 / 002-OQ-002 / 003-OQ-010 — foreign error strings as public contract.** ★

This is the single largest cost in the spec set and it should be decided once. Across the
three specs, roughly 25 byte-exact requirements embed a string owned by CPython, libc or
`jsonschema`:

- `could not convert string to float: 'abc'`
- `float() argument must be a string or a real number, not 'NoneType'`
- `timestamp out of range for platform time_t` *(libc, macOS-specific wording)*
- `'in <string>' requires string as left operand, not int`
- `utf_8_encode() argument 1 must be str, not int` *(a CPython internal function name)*
- `unterminated character set at position 0`
- `Expecting value`, `Unexpected UTF-8 BOM (decode using utf-8-sig)`
- `'zeta' is a required property`, `'' should be non-empty`, `5 is not of type 'object'`
- `[Errno 2] No such file or directory: '<path>'`

None is documented; none is tested on the Python side; all can change with an interpreter or
library upgrade; and a Rust implementation must hardcode every one. The three options are
(a) freeze them in a TermProof-owned table with Python-side tests pinning each, (b) define
TermProof's own vocabulary and change Python to emit it, or (c) declare them behavioural and
require only `passed`, plus the TermProof-authored prefix, to match.

### Recipe format (001)

| | Question | ★ |
|---|---|---|
| OQ-002 | `cols: 80.9` silently truncates to `80` at load while the validator rejects it | |
| OQ-003 | `jsonschema`'s message strings as public contract (see above) | ★ |
| OQ-004 | `argv: []` loads but cannot run; the schema says `minItems: 1` | |
| OQ-005 | Booleans coerce to numbers everywhere except `recipe_version`, where `bool` is explicitly excluded | |
| OQ-006 | The validator tolerates `{"priority": null}` and the loader then stores `None` in a string field | |
| OQ-007 | A non-object `renderers` fails two different ways in two layers | |
| OQ-008 | `source_path` is in the model but not in the format | |
| OQ-009 | The loader's failures are leaked exceptions (`KeyError: 'command'`), not messages | |
| OQ-010 | `validate_recipe_file` raises `FileNotFoundError` instead of reporting an issue | |

### Built-in steps (002)

| | Question | ★ |
|---|---|---|
| OQ-001 | Only `wait_for_regex` validates its duration; the other four inputs accept NaN, zero and negative | ★ |
| OQ-002 | CPython and libc error text as contract (see above) | ★ |
| OQ-003 | `stable for nans`, `stable for infs`, `stable for -1.0s` are accidents | |
| OQ-004 | `wait_for_idle`'s "no output" branch is a buffer-length heuristic, not a state | |
| OQ-005 | `ctrl-<unmapped>` silently sends nothing, and the behaviour depends on the `ptyprocess` version | ★ |
| OQ-006 | `press` uses Unicode case folding where ASCII would do | |
| OQ-007 | Missing-key diagnostics are bare `str(KeyError)`: `'key'`, `'text'`, `'f13'` | |
| OQ-008 | A step with no `action` kills the run; the replacement diagnostic is undecided | ★ |
| OQ-009 | A closed `match` cannot dispatch plugin-registered actions | |
| OQ-010 | `send_text` requires `text`; `send_line` defaults it | |

### Built-in assertions (003)

| | Question | ★ |
|---|---|---|
| OQ-001 | Eight malformed inputs abort the whole run and discard `result.json`; replacement diagnostics undecided | ★ |
| OQ-002 | An explicit `exit_code` assertion does not suppress the synthetic one, so the run can never pass | |
| OQ-003 | `exit_code`'s detail is type-blind: `'0'` vs `0` fails while printing `expected 0, got 0` | |
| OQ-004 | A contains-assertion's detail is identical whether it passed or failed | |
| OQ-005 | The `detail` override works on four of eight assertions, and `""` silently does nothing | |
| OQ-006 | `file_exists` does not collapse `..` | |
| OQ-007 | `file_contains` reads a missing file as `""` and so cannot report one | |
| OQ-008 | Python's JSON decoder accepts `NaN`/`Infinity`/duplicate keys, so "is valid JSON" is not | |
| OQ-009 | `json_schema` never states which JSON Schema draft it validates against | |
| OQ-010 | `jsonschema` messages and its `best_match` heuristic as contract (see above) | ★ |

---

## 6. Where a spec supersedes the oracle

Constitution Principle III makes the Python implementation the oracle *until a spec supersedes
it*, and requires superseding to be a deliberate, written act. Two requirements do so. Both
need a corresponding fix on the Python side, and both leave the diagnostic text undecided:

- **002-FR-025** — a step with no `action` must not kill the run. Python raises
  `KeyError: 'action'` from inside the runner's own error handler. Text: 002-OQ-008.
- **003-FR-020** — no assertion input may abort the run. Python raises for eight distinct
  inputs, none of them caught anywhere between the assertion and the CLI, so the user gets a
  traceback and no `result.json`. Text: 003-OQ-001.

Everything else in the spec set describes the oracle as it is, including behaviour flagged as
accidental.

---

## 7. What the conformance harness needs from this

Issue #3 (RUST-026) is the consumer. The acceptance scenarios in all three specs are written
as concrete input plus concrete expected output so they can be encoded as corpus cases without
interpretation. Three constraints carry over from how these observations were made:

1. **Use a real child process for steps.** A stub session records `send_text {text: 5}` as
   passing. This mistake has now been made three times in this project's history.
2. **Diff `detail` as a literal string, not a substring or a regex.** Every parity failure the
   review found was a detail mismatch, and most were one character.
3. **Substitute the fixture root before diffing paths**, and never let a case depend on
   wall-clock timing or on filesystem case-sensitivity.
