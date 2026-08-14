# TermProof Rust Port Constitution

This constitution governs `md-mt/termproof-rust`, a reimplementation of the TermProof
Python verifier. It is not a generic engineering-values document. Every principle below
was paid for by a specific, documented failure in this repository's own history, and each
one names that failure so a future reader can tell whether the principle is still earning
its place.

## The failure this constitution exists to prevent

Between 2026-07 and 2026-08 the port merged the step engine (#145), proposed the assertion
engine (#146), and shipped a conformance gate — each titled "with corpus parity", each with
a green local test suite, and each reviewed as sound. An adversarial review then built a
differential harness that drove identical inputs through the Python oracle and the Rust
implementation, and measured:

| layer | cases | agreed | diverged | pass/fail flips | panics |
|---|---|---|---|---|---|
| steps | 110 | 26 | 84 | 18 | 6 |
| assertions | 107 | 29 | 78 | 15 | 0 |

The tests passed because they asserted against strings the author had written, not against
the oracle's output. The same root error — believing Rust's `{:?}` is Python's `repr()` —
was reasoned about, written into an explanatory comment, and got wrong twice, three weeks
apart. Six of the divergences were process-killing panics reachable from a user's recipe
file.

The root cause was not carelessness. It was that **parity was asserted against an oracle
nobody had written down.** "Matches the Python implementation" is unfalsifiable until
someone states what the Python implementation is supposed to do.

## Core Principles

### I. No capability without an executable check that fails when it is absent (NON-NEGOTIABLE)

A capability is claimed only when a check exists that would go red if the capability were
removed. Before landing a check, delete or stub the thing it covers and confirm it fails.
A check that passes against a stub is not evidence — it is decoration.

*Paid for by:* the conformance gate whose eight cases all asserted `exit_code: 0` and
`stdout_contains: ["termproof"]`, and which a Rust binary printing one constant string for
every argv satisfied 8/8. And by the step dispatch table whose seven match arms were
exercised for one action, so a typo in any of the other six would have shipped silently.

### II. Parity is measured, never asserted (NON-NEGOTIABLE)

A parity claim is admissible only with a differential run attached: identical inputs driven
through both runtimes, diffing pass/fail **and** the diagnostic text, with the case count
and the divergence count stated. Reading two implementations side by side and finding them
similar is not measurement — the review found 162 divergences in code that had passed
exactly that inspection.

Corollaries:

- The words "parity", "byte-for-byte", and "byte-stable" do not appear in a PR title or
  body without a measurement in the same document.
- Self-reported gate results must use CI's exact command. A narrower command that passes is
  a false green: `cargo clippy -p termproof` exits 0 where CI's
  `--workspace --all-targets --all-features` fails.
- Divergences are reported as a count out of a total, never as a list of the ones found.

### III. The Python implementation is the oracle until a spec supersedes it

Where no spec covers a behaviour, the Python implementation defines it, including behaviour
that looks accidental. Where a spec in `specs/` covers it, **the spec wins** and the Python
side is the thing that may need to change.

This is how the port stops chasing source code. A divergence from Python is a defect only
if it also diverges from a spec, or if no spec covers it yet. Superseding the oracle is a
deliberate, written act — an amendment to a spec, with a rationale — never an incidental
consequence of a rewrite.

### IV. Specify by evidence ladder, and name the rung

Behaviour is derived in this order, and the source is named in the spec:

1. Documented contracts — the recipe JSON schema, `docs/recipe-format-v1.md`,
   `docs/plugin-protocols.md`.
2. Observable behaviour — the Python CLI or module run against real inputs.
3. The oracle's tests — they encode intent and the reason for it.
4. The oracle's source — last resort, and flagged as such in the spec.

A spec written from source promotes the implementation's accidents to requirements, which
is how a rewrite inherits the bugs it was meant to shed. When source is the only available
authority, the spec says so, and anything that looks like an accident rather than a
decision is raised as an open question instead of being frozen.

*Paid for by:* two harnesses that copied a fake session's `send_text` body from the
implementation rather than exercising a real child process, and so recorded
`send_text {text: 5}` as *passing* in Python. Against a real `pexpect` child it raises
`TypeError`. The port and its reviewer inherited the same wrong oracle.

### V. Every requirement is marked behavioural or byte-exact, with its authority named

This project has two kinds of requirement, and conflating them is how the parity argument
restarts:

- **`[BEHAVIOURAL]`** — what the tool must do. A reasonable reimplementation could satisfy
  it differently.
- **`[BYTE-EXACT]`** — the literal bytes *are* the requirement, because users and downstream
  tools depend on them: the recipe schema, `result.json` field names and shapes, exit codes,
  and any diagnostic string a test or consumer matches on.

For a reimplementation some literal strings genuinely are the contract, which is the
opposite of how a spec normally treats them. Every `[BYTE-EXACT]` requirement names its
authority — the JSON schema, a documented format, or an observed Python run with the
observation recorded — so the claim is checkable rather than asserted.

### VI. Contain failures; a recipe must not be able to crash the runtime

Input reachable from a recipe file must produce a structured failed result, never a panic,
an abort, or a silently dropped value. Fallibility is expressed in types at the boundary,
not by `unwrap`, and not by trusting a validator that ran somewhere else.

*Paid for by:* `Duration::from_secs_f64` and `Instant::now() + timeout` panicking on
`timeout_seconds: 1e300`, taken straight from a user's recipe, where Python returns an
ordinary failed result. `1e18` was fine and `1e19` was not, and the boundary was invisible
to a recipe author.

### VII. Unreachable code is unverified code

A module is not "done" while nothing calls it. Wire each module into a real execution path
before starting the next one.

*Paid for by:* ~1,400 lines of "verified" step and assertion code with no caller anywhere in
the workspace. `grep "impl ExecutionContext for"` returned nothing; `run_steps` and
`evaluate_all` had no callers outside their own tests. Every unreachable module makes the
eventual wiring commit larger and defers all of its parity surprises to the same day.

### VIII. Internal representations never reach a user artefact

`{:?}` is not `repr()`. Rust `Debug` output — `UnknownKey("f13")`, `Some(Bool(true))`,
`BadCtrl("ctrl-")` — must not appear in `detail`, in `result.json`, or in a report. Neither
must a dependency's multi-line diagnostic: the `regex` crate's parse errors are ASCII art
with embedded newlines, and they were being pasted verbatim into a report field.

Where a diagnostic must match Python's, it is produced by a shared, tested formatter, not by
an ad-hoc `format!` at each call site. Python's `repr` rules — single quotes, switching to
double quotes when the value contains an apostrophe and no double quote, the trailing comma
on a one-element tuple, the `\xNN`/`\uNNNN` escape ladder — are a formatter, and this project
needs exactly one of them.

## Evidence Standards

These apply to any claim made in a PR body, a commit message, an issue comment, or a spec.

- **State what was run.** The command, the revision, and the result. "Tests pass" is not a
  result; `cargo test --workspace` → 99 passed is.
- **Absence of CI is not a green board.** Two of the PRs above had *zero* workflow runs ever
  recorded for their head branch — not pending, not failed, none — while the board looked
  clean. Before reading a signal, confirm the signal ran.
- **A gate that fails on first use in a clean environment will be waived, not fixed.**
  Bootstrap noise (a package manager writing to stderr) is a gate defect, not flake.
- **Name what was not checked.** A report listing only what was verified reads as
  completeness. If a surface was skipped, say which and why.
- **Coverage is reported for new code, not for the workspace.** A workspace-level gate
  absorbs a 74%-covered new module without moving. Baselines carry a `generated_at`
  timestamp; a crate absent from the baseline is a gate failure, not a trivial pass.

## Specification Discipline

- Specs live in `specs/NNN-short-name/spec.md` and follow the Spec Kit template.
- Every functional requirement carries `[BEHAVIOURAL]` or `[BYTE-EXACT]` and an **Authority**
  naming the rung of the ladder in Principle IV that it came from.
- Acceptance scenarios are written so a differential harness can execute them: concrete
  inputs, concrete expected outputs, no prose conditions.
- Ambiguity in the oracle is recorded as an open question, never resolved by silently picking
  whatever the current implementation happens to do. **Finding these is a success.** A spec
  set reporting no open questions against a codebase like this one has not looked.
- Observations that did not become requirements — literals deliberately left out, behaviours
  judged to be accidents, version-dependent output — are recorded in
  `specs/OBSERVATION-LOG.md` so the omissions are auditable as choices rather than gaps.

## Governance

This constitution supersedes convenience, precedent, and PR urgency. It does not supersede a
spec in `specs/`; where the two disagree about a specific behaviour, the spec governs that
behaviour and the conflict is itself a defect to be fixed here.

- Amendments require a PR stating the failure or evidence motivating the change. A principle
  may be removed when the failure it names can no longer occur — and the removal must say
  why.
- Reviews check compliance explicitly. A PR asserting parity without a measurement is
  incomplete regardless of how correct its code is.
- Principles I, II, and VI are non-negotiable: they may be amended, but not waived for a
  particular change.

**Version**: 1.0.0 | **Ratified**: 2026-08-11 | **Last Amended**: 2026-08-11
