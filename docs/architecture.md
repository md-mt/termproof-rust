# Architecture

How the workspace is put together: the crates, the boundary to the Python
oracle, the session abstractions, and what happens when `termproof run`
executes a recipe. Written for someone reading the code, or deciding whether
to build on the library.

## Workspace and crates

The workspace has three crates, one of which publishes:

| Crate | Role | Publishes |
|---|---|---|
| `termproof` | the whole library: recipe model, schema, steps, assertions, orchestration, terminal sessions, evidence pipeline | yes |
| `termproof-cli` | the `termproof` binary: command parsing, diagnostics | `publish = false` |
| `termproof-plugin-protocol` | the versioned newline-delimited JSON protocol for out-of-process plugins, with client/host support | `publish = false` |

See [`publishing.md`](publishing.md) for why the two held-back crates stay
held.

`termproof` is the interesting one. It was merged from three crates —
`termproof-core`, `termproof-terminal`, `termproof-evidence` — before any of
them was published, and the shape reflects that history:

- **The crate root** is what was `termproof-core`: the recipe model
  (`models`, `recipe`, `config`), schema (`schema`, `validation`, `pyschema`),
  the built-in steps and assertions, planning and orchestration
  (`planner`, `runner`, `execution`, `agent`), artifact storage and caching
  (`store`, `cache`), run results (`result`), and the `py*` compatibility
  shims (`pyregex`, `pyrepr`, `pypath`) that keep the port's behaviour close
  to the Python oracle's.
- **`terminal`** is what was `termproof-terminal`: PTY, tmux and process
  sessions, plain and attributed screen state, asciicast recording, idle
  detection, and the `Session`/`SessionBackend` implementations.
- **`evidence`** is what was `termproof-evidence`: screenshot and video
  rendering, Markdown reports, visual baselines, diff, dedup and upload.
- **`junit`** is the JUnit XML writer. It was in `termproof-evidence` too, and
  `evidence::report::generate_junit` still resolves, but it reads a
  `RunResult` and renders nothing, so it is its own module and its own
  feature (`junit`), independent of `evidence` in both directions.

The crate is flat rather than nested under a `core` module: the root is the
primary surface, and a module named `core` would shadow the `core` crate for
every path in its scope. The two nested modules keep their own re-exports so
`crate::error` and `terminal::error`, and `crate::result` and
`evidence::report`, stay distinguishable.

## The boundary to the Python oracle

The Python implementation at `md-mt/termproof` is the shipped product and the
behavioural oracle. The port is measured against it, not merged with it:

- **The recipe schema and the example corpus stay with the Python
  repository.** They are the contract both implementations answer to.
  `load_canonical_schema` in `termproof` therefore finds nothing in this
  checkout. What the crate does pin is its *own* generated schema, to a
  checked-in snapshot (`tests/snapshots/recipe_schema_v1.json`, guarded by
  `tests/schema_snapshot.rs`) — a local structural stability check, not a
  parity claim.
- **The differential harnesses** (`harness/`) record the oracle's verdicts
  over checked-in corpora and replay the same cases through the port. They
  assert a floor rather than equality, and the counts and residual
  divergences are documented in `docs/status-and-parity.md` and
  `harness/README.md`. CI runs them as part of `cargo test --workspace`.
- **The `py*` shims** are the port's answer to behaviours that are really
  CPython's or libc's — regex dialect, `repr` rendering, path semantics,
  schema error selection. They are where the port gets closest to the oracle
  without copying its error strings wholesale.

## Sessions: two abstractions that point in opposite directions

There are two ways to talk to a running program, and they serve different
authors:

- **Implement `Session` to write a backend.** The trait is narrow and totally
  fallible — that is what makes a backend cheap to write. Implementations:
  `PtySession` (a child on a real pseudo-terminal via `portable-pty`, the
  default), `PtySessionBackend` (what a run uses by default), tmux (a pane
  whose grid is read back with `capture-pane`), process, plugin, and
  `InMemorySession` for tests.
- **Use `SessionDriver` to write a scenario.** `terminal::driver` is a
  scenario-facing wrapper over `Box<dyn Session>` that supplies default
  timeouts, a `screen_contains` convenience, and defers errors so a failed
  keystroke is reported once at the assertion — naming the call that first
  failed — instead of at every `?`.

The `SessionBackend` implementations construct sessions from config; a
`SessionDriver` sits on top of whatever session a backend produced.

## What happens on `termproof run`

1. **Discover.** `termproof-cli` scans the paths given for files ending in
   `.recipe.json`, `.recipe.yaml` or `.recipe.yml` (`run.rs`). It loads each
   file through `LoadedRecipe::from_file`, which reads the recipe model and
   its renderer table (renderer name → extra argv, sorted by name so plans are
   reproducible).
2. **Plan.** `planner::plan_items` expands recipe × renderer into
   `PlanItem`s, sorted by `(recipe_name, renderer)` so parallel execution is
   deterministic regardless of filesystem order. `run_parallel` executes them
   with bounded workers and collects results in plan order.
3. **Execute.** `Runner` (via `execution::ScriptedPtyMode`) runs each recipe
   against a real child on a pseudo-terminal through `PtySessionBackend`.
   Steps drive the session: `send_text`, `send_line`, `press`, `wait_for_text`,
   `wait_for_idle`, `wait_for_exit`, `sleep`. Only `execution: scripted` on a
   pty runs; anything else is refused with a diagnostic naming the reason —
   running it on a pty anyway would report a verdict about something the
   recipe did not ask for.
4. **Assert.** The eight built-in assertions are evaluated against what the
   target actually did — screen text, raw output, exit code, JSON schema.
   `Runner` supplies no `evaluate_assertion` of its own; what runs is the
   trait's default body, `assertions::evaluate`.
5. **Store and report.** `store::new_run_dir` builds a unique, sanitized run
   directory under the output root (default `.termproof/runs`), with a
   path-traversal guard and atomic writes. The run writes `result.json`,
   `report.md`, `raw_output.txt`, `screen.txt` and an asciicast per run, plus
   `latest-report.md` — and a JUnit file when `--xml-path` is given.
   `cache` provides a content-addressed run cache for `--skip-unchanged`.

## Library surfaces not wired into `termproof run`

A run writes `raw_output.txt`, `screen.txt` and a cast if one was recorded —
no image at all. The following are tested library APIs with **no caller in
the CLI**: treat them as a library a caller could build on, not as behaviour
the CLI has.

- `parity` — compares two runs and reports where they disagree.
- `before_after` — reports which outcomes flipped.
- `selection` — maps a changeset onto recipes via `ci_paths`.
- `run_config` — a whole run described by one file.
- `vocabulary` — a configurable failure detector.
- `build_info` — provenance for the binary under test.
- `terminal::attributed` — a per-cell screen (colours, styles, display
  width) with an SVG renderer.
- `terminal::tmux` — a `Session` that runs the program in a tmux pane and
  reads the grid back with `capture-pane`.
- `terminal::proc` — child processes with a deadline.
- `terminal::driver` — `SessionDriver` (see above).
- `evidence::screenshot` / `evidence::cast_video` — stills and video frames
  through one renderer.
- `evidence::dedup` — skips re-rendering a screen identical to the step
  before it.
- `evidence::uploader` — a publishing seam with a fallback chain.
- `evidence::collector` — `EvidenceCollector`, the ordered step model those
  plug into; `publish` renders, dedupes, uploads and writes an
  `evidence.json` manifest in one pass. It sits **beside** `RunResult` rather
  than inside it — join the two with `manifest.attach_to(&mut result)`.

`docs/status-and-parity.md` carries the same list from the status angle
(what exists versus what a run does), and `conditional-recipes.md` explains
why recipes stay linear — a consumer with a branching scenario builds its own
runner on `SessionDriver` rather than asking the recipe format for `when`.

## Further reading

- [`docs/status-and-parity.md`](status-and-parity.md) — measured agreement,
  known divergences, and what a run still cannot do.
- [`docs/rust-reimplementation-spec.md`](rust-reimplementation-spec.md) — the
  design rationale, compatibility contract and parity gates the port is
  measured against. Written before the split, so parts describe a workspace
  under `rust/` in the Python repository; its header says which sections are
  superseded.
- [`docs/engineering-baseline.md`](engineering-baseline.md) — formatting,
  lint, error, tracing, dependency, feature and unsafe-code policy.
- [`docs/conditional-recipes.md`](conditional-recipes.md) — why the recipe
  format stays linear, what a consumer with a branching scenario uses instead.
- [`harness/README.md`](../harness/README.md) — the differential harnesses,
  the corpora, and how to regenerate expectations.
