# Recipes that branch on observed state

**Decision: out of scope. The recipe format stays linear, and this crate does
not offer a second way to say what a recipe does.** A scenario that branches on
what it observes belongs in the consumer's own runner, written against
`termproof::terminal` and `termproof::evidence` and — if it wants everything
downstream of a run — producing a `termproof::result::RunResult` itself.

Raised as issue #16. This document is the answer, written down so a consumer
does not have to rediscover it, and so the next person to propose branching
starts from the reasoning rather than from scratch.

---

## The question

`Recipe` is a serde struct: a command, an ordered list of steps, and a list of
assertions. It describes a scenario that runs the same way every time. It cannot
describe one that decides what to do next from what it just saw.

Three shapes come up in practice:

1. **Poll, then act.** Wait for a status alert to render, then press Enter only
   if it did.
2. **An overlay that may or may not appear.** A permission prompt that must be
   dismissed if it shows up, and ignored if it does not.
3. **Retry.** Re-drive a step whose precondition is racy.

Three ways to answer, all of them proposed on the issue:

- **A trait alongside the struct** — `trait ImperativeRecipe { fn run(&self, …)
  -> RunResult; }`, and a `Runner` that accepts either that or a `Recipe`.
- **A `when` predicate on a step** — evaluated against the screen, so the
  declarative format grows a conditional rather than the project growing a
  second model.
- **Neither**, documented.

---

## The three shapes, and what each one actually needs

The options are easier to judge after working out what the scenarios need,
rather than after assuming they all need "a branch".

### 1. Poll, then act

This one splits in two depending on whether the alert is *required*.

If it is required, there is no branch. `wait_for_text` already carries a
deadline, and its failure is the verdict the recipe wanted:

```json
{"action": "wait_for_text", "text": "Deploy complete", "timeout_seconds": 10}
{"action": "press", "key": "enter"}
```

The conditional in the consumer's code is defensiveness against a wait that the
consumer's own runner did not have. The linear model expresses this today, and
the differential harness measures it.

If the alert is genuinely optional — the run should carry on either way — then
the branch is real. But notice what such a recipe now claims: it passes whether
or not the alert rendered, so it has asserted nothing about the alert. For an
evidence-first tool, a step that is allowed to not happen is a step that proves
nothing, and folding two outcomes into one verdict is the specific failure this
project exists to make visible. The honest shape is usually two recipes, one per
branch, each asserting its own branch — which the linear model already
expresses, and which the harness can measure, and which a report can tell apart.

**So: mostly already covered, and where it is not, a branch is the wrong fix.**

### 2. An overlay that may or may not appear

This is the one none of the linear model's tools reach, and it is not a
positional problem. A permission prompt can interrupt between any two steps, or
*during* a step's own wait loop. "Where in the script does it appear" has no
answer.

A `when` predicate covers it only by guarding every position: a conditional
dismissal step between each pair of real steps, `n + 1` guards for `n` steps,
and still wrong when the prompt arrives mid-wait rather than between two steps.

This is exactly the wall Playwright hit, and it is worth being precise about
what it did rather than about what the shape of the answer feels like.
Playwright 1.42 added `page.addLocatorHandler(locator, handler)`: you register a
handler against a locator, and Playwright invokes it when that locator becomes
visible *during an actionability check it is already performing for some other
action*, then retries the original action. `times` caps how often it may fire;
`noWaitAfter` relaxes the default expectation that the overlay is gone once the
handler returns; `page.removeLocatorHandler` unregisters it. Playwright did not
extend its declarative surface to express the overlay, and it did not tell
callers to drop to raw imperative code either.

The precedent's actual content is: **the trigger is a fact about the wait loop,
not a position in a script, so the mechanism belongs where the wait loop is.**
Read that way it argues for neither of the two options on the table. A handler
of this shape would hang off the session layer — a pattern, an action, a bound
on invocations, consulted by the wait loops `termproof::terminal` already
runs — and it would add no second recipe model and would not touch the recipe
format at all.

Such a handler is not proposed here: it is a different mechanism from either
option and belongs to a different layer, so it wants its own issue rather than
a paragraph in this one. Recording the shape a future answer should take is the
useful part. In the meantime the overlay is dismissible today, because
`terminal::SessionDriver` is where a consumer's own wait loop already lives —
`screen_contains` before each driving call, or after a `wait_for_text_within`
that is allowed to fail, is the hand-rolled version of the same thing.

**So: `when` mangles it; the mechanism that actually fits is not a recipe
construct; and a consumer can write the loop today against the driver.**

### 3. Retry a step whose precondition is racy

A branch is not what this needs. A loop is, and a predicate language without
iteration does not have one.

Adding `retry: 3` to a step is a separate feature with its own unanswered
questions, none of which is a predicate: is the existing `timeout_seconds` per
attempt or across all of them? Does the run write one screen for the step or
one per attempt? Does a step that passed on the third attempt pass? Each answer
is a decision about what a step *is*.

And the usual cause is worth naming: a racy precondition is most often a missing
wait rather than a missing loop, and every `wait_for_*` step already carries a
deadline. Playwright's answer here is likewise not a predicate — auto-waiting
removes most retries, and what remains is `expect.toPass()`, which wraps a
*block*. The residue where a step genuinely should be re-driven is real, but it
is iteration, and iteration is not on offer from any of the three options.

**So: `when` cannot express it at all.**

### Tally

| | poll-then-act | optional overlay | retry |
|---|---|---|---|
| Linear model today | mostly covered | no | no |
| `when` predicate | covered | mangled — positional guard for a non-positional event | **cannot express** |
| Trait | covered | covered, by hand | covered, by hand |
| Neither | consumer's runner | consumer's runner | consumer's runner |

A `when` predicate handles one of the three, and the one it handles is the one
the linear model mostly already covers. That is the whole case against it before
any argument about expression languages is needed.

---

## What the differential harness can and cannot measure

This is the argument that decides it, and it is specific to this project rather
than a general preference for small APIs.

A recipe is a JSON document, and that is precisely what makes the differential
harness possible: hand the same document to both runtimes and compare the
verdicts. Every claim this port makes about its own correctness is a count from
that comparison. The README says so in as many words — *until a parity gate
passes, treat the Python implementation as the only authority*.

**A `when` key does not fail against the oracle. It is silently ignored by it.**
`additionalProperties: true` is in the schema (spec 001, FR-013), unknown step
keys are preserved rather than rejected, and the Python step dispatcher reads
only the keys its `action` names. So one file would run its step unconditionally
over there and conditionally over here, with no error on either side. That is
worse than an unmeasurable feature: it makes an already-measured document format
mean two different things. The harness would report the port as diverging, and
it would be right.

**A trait is not a document at all.** Nothing can be handed to the oracle, so
agreement is not merely unmeasured — it is undefined. The project would acquire
a second execution surface whose only correctness evidence is its own unit
tests, which is the exact standard the repository tells you not to accept from
it.

**Neither costs the harness nothing.** The measured surface stays the measured
surface.

Two limits on that argument, so it is not over-read:

- It is not an argument against new surface generally. `termproof::terminal` and
  `termproof::evidence` grow without oracle coverage and that is fine, because
  the harness measures the step and assertion layers and never claimed to
  measure those (`harness/README.md`, "What the corpus does and does not
  measure"). The objection is specific to a *second way of saying what a recipe
  does*, because the recipe is the artefact the two runtimes share.
- It is contingent, not permanent. If the oracle grows branching, branching
  stops being unmeasurable and starts being something the port must match. See
  "What would reopen this".

---

## What each answer commits the project to

**A trait commits it to two recipe models, permanently.** The cost is not the
trait; a trait is ten lines. The cost is `Runner` accepting either, and every
consumer of `Recipe` downstream of it acquiring a second case: `selection`
(which maps a changeset onto recipes via `ci_paths`), `planner` (recipe ×
renderer), `validation`, `schema`, `run_config`, `before_after`, and the CLI's
discovery of recipe files under a path. Several of those have no answer rather
than a second answer — there is no JSON Schema for a Rust closure, no `ci_paths`
to select on, nothing for `RunResult::score_from_assertions` to score — so they
become "not supported for imperative recipes" footnotes that never go away.
Additive at the type level; permanent at the design level.

**A `when` predicate commits it to an expression language inside a JSON
struct.** `screen_contains` is the first version. The next three requests are
negation, disjunction, and a capture to compare a later step against, and there
is no principled place to stop, because each one is individually reasonable. The
destination is known and this project does not want to arrive at it by
accident.

**Neither commits it to nothing that has to be undone.** No format has shipped
that would have to be kept working. If branching is added later, it costs then
what it would cost now, and it can be added in whatever shape the evidence then
supports — including a shape none of the three options describes.

---

## What a consumer does instead

Keep your own runner. Use this crate for the layers it is actually measured at,
and for the seams that do not require it to have run anything.

- **`termproof::terminal::SessionDriver`** — the scenario-facing wrapper over
  `Box<dyn Session>`, and where a branching scenario should start. It supplies
  default timeouts, `screen_contains` / `raw_contains`, and deferred errors, so
  a failed keystroke is reported once at the assertion — naming the call that
  first failed — rather than at every `?`. A scenario written against it is
  ordinary Rust and can branch, loop and retry however it likes. Implement
  `Session` to write a *backend*; use `SessionDriver` to write a *scenario*.
- **`termproof::terminal`** more broadly — PTY, tmux and process sessions,
  plain and attributed screen state, asciicast recording, idle detection.
- **`termproof::evidence`** — screenshot and video rendering, dedup, Markdown
  and JUnit reports, visual baselines, diff and upload.
- **`termproof::result::RunResult`** — the seam worth knowing about. It is a
  plain struct with public fields, so a consumer's own runner can build one.
  Everything that consumes a `RunResult` then works on it: `parity::compare` for
  cross-implementation and before/after comparison, `before_after` for which
  outcomes flipped, the reporters, the JUnit writer, the uploader. **This crate
  does not have to have run the scenario in order to compare, report and publish
  it.**

All three of shape 1, 2 and 3 are a few lines against the driver. The optional
alert, with the detail that matters:

```rust
use std::time::Duration;
use termproof::terminal::SessionDriver;

let mut driver = SessionDriver::new(Box::new(session));

// `wait_for_text` treats absent text as a failure — waiting for something is
// asserting it happens. When the alert is genuinely optional, that verdict is
// not wanted, so clear it and ask the screen directly.
driver.wait_for_text_within("Deploy complete", Duration::from_secs(10));
driver.clear_failure();

if driver.screen_contains("Deploy complete")? {
    driver.press("enter");
}

driver.expect_screen_contains("Ready")?;
```

The `clear_failure` line is the whole of the branch, and it is deliberately
explicit: turning a failed wait into "carry on" is a decision the scenario
makes in the open, which is the property a `when` predicate would have hidden
inside the recipe.

The boundary this decision draws is not "you are on your own". It is: *we do not
run your branching scenario; we do everything on either side of it.*

One piece of work is still in flight, and nothing in this decision depends on
it: issue #15, the per-step evidence collector. Until it lands, a consumer that
wants per-step artefacts renders them through `evidence` itself and owns its own
artefact layout.

---

## What would reopen this

A decision recorded without its reversal conditions is just an opinion with a
date on it. Any of these should reopen it:

1. **The oracle grows branching.** If the Python implementation adds a
   conditional construct to the recipe format, it stops being unmeasurable and
   becomes something this port must match. The ordering matters: the format
   belongs to the oracle, so this is the cleanest reopener and the port should
   not lead.
2. **A handler on the session rather than a construct in the recipe.** Shape 2
   above has an answer that costs neither of the other options' prices, because
   it lives below the recipe layer. Now that `SessionDriver` has landed there is
   somewhere obvious for it to hang, so it deserves its own issue against
   `termproof::terminal` — registered on the driver, consulted by the wait loops
   it already runs. It is not blocked by this decision, and a consumer does not
   have to wait for it: the hand-rolled version is the `screen_contains` check
   shown above.
3. **Evidence that the split costs more than it saves** — consumers routinely
   reimplementing step dispatch in order to get branching, and their
   reimplementations drifting from the built-ins. That would mean the boundary
   is in the wrong place, and it is observable rather than a matter of taste.
