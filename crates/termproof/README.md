# termproof

Evidence-first verification for TUI and terminal applications — the library
half of [TermProof](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> and
> [docs/status-and-parity.md](../../docs/status-and-parity.md)
> before depending on this crate.

## Layout

This crate was merged from three — `termproof-core`, `termproof-terminal` and
`termproof-evidence` — before any of them was published, so the shape below is
the only one that has ever existed on crates.io.

- **The crate root** is the recipe model, config, schema, validation, steps,
  assertions, planning, orchestration and execution. It is flat rather than
  under a `core` module: it is the crate's primary surface, and a module named
  `core` shadows the `core` crate for every path in its scope.
- **`terminal`** is the session layer — PTY, tmux and process sessions, plain
  and attributed screen state, asciicast recording.
- **`evidence`** is the evidence pipeline — screenshot and video rendering,
  Markdown reports, visual baselines, diff and upload.
- **`junit`** is the JUnit XML writer. It lived in `termproof-evidence` too, and
  `evidence::report::generate_junit` still resolves, but it reads a `RunResult`
  and renders nothing, so it is its own module and its own feature.

## Features

All four are on by default, so `termproof = "0.3"` is the whole crate — the
shape that has always been published.

| Feature | Enables | Costs |
|---|---|---|
| `evidence` | the `evidence` module | `image`, `avt` |
| `junit` | the `junit` module, and `generate_junit` in `evidence` | `quick-junit` |
| `json-schema` | `validation`, `pyschema`, and the `json_schema` built-in assertion. Implies `schema` | `jsonschema` |
| `schema` | the `schema` module, and `JsonSchema` on `Recipe`, `VerifierConfig` and the types they contain | `schemars` |

Turning all four off takes the crate from 180 transitive dependencies to 66:

```toml
termproof = { version = "0.3", default-features = false }
```

That build still has the whole session layer and the other seven built-in
assertions; what it loses is the evidence pipeline, JUnit output, JSON Schema
validation and schema generation.

`junit` is the only feature that neither implies another nor is implied by one.
It was proposed as `junit = ["evidence", ...]`, but `generate_junit` takes a
`&[RunResult]` and reads nothing else, so making it depend on `evidence` would
have charged a JUnit-only consumer `image` and `avt` for nothing. Either half
can be had without the other, and each costs only its own crates:

```toml
# stills, video and Markdown, no JUnit — six crates lighter than 0.3.1's
# `evidence`: quick-junit, quick-xml, newtype-uuid, uuid, strip-ansi-escapes, vte
termproof = { version = "0.3", default-features = false, features = ["evidence"] }

# JUnit, no renderers — those same six crates and nothing else, fifteen lighter
# than asking for both
termproof = { version = "0.3", default-features = false, features = ["junit"] }
```

`schema` is the small one — six crates, against `json-schema`'s 87 — and it is
not off by default and not off for size. It exists because `schemars` is the
only dependency that reaches this crate's *public API*: the derives put
`JsonSchema` on published types, so a consumer already on a different `schemars`
major cannot deduplicate by pinning the way it can for everything else here, and
has to carry both. Turning `schema` off means this crate does not name
`schemars` at all, and that second copy is gone.

There is no `terminal` feature. The crate root is built on `terminal` and
nothing in `terminal` depends on the root, so a build without it would not be
this crate; it would also save nothing, since every build drives a terminal.

## Dependency floors

Every version requirement here is a floor — the lowest version this code
compiles and passes the differential harnesses against — and CI runs the suite
at each one, so a floor that stops being true fails the build. If you already
pin one of these elsewhere, two of them will not move, and the reason is not
guessable from the import lists:

- **`portable-pty` 0.9, not 0.8.** Every name `terminal::pty` imports exists in
  0.8 too. What does not is `ExitStatus::signal()`, added in 0.9.0 and read by
  `PtySession::exit_signal`; at 0.8.1 the signal name is a private field, and
  that is the *only* compile error at 0.8. It is a choice, not an
  impossibility — 0.8 still shows the name through `Display`, and scraping it
  back out returns the same value — but a `Display` format carries no semver
  guarantee, so that version would answer `None` after a reword with nothing
  to catch it. The floor buys one public accessor; nothing else in the crate
  reads it, and it is not on the `Session` trait.
- **`unicode-width` 0.2, not 0.1.** `vt100` 0.16 requires `^0.2.1`, and this
  crate's own width calls have to consult the same table as the `vt100` grid
  they describe or the two disagree about which column a wide glyph occupies —
  which moves glyphs in rendered SVGs and changes
  `AttributedScreen::render_fingerprint`, so evidence dedup stops recognising
  two identical screens as identical. Forcing 0.1 splits the graph in two and
  fails three tests in `terminal::attributed` that exist to catch exactly that.
  Widening the requirement to `>=0.1, <0.3` would resolve to 0.2 anyway, so it
  would buy nothing and only make the floor untestable.

Everything else is a plain floor with no such constraint; `Cargo.toml` carries
a comment on each one that sits above the oldest workable version.

## What it provides

### Root — recipes, steps, assertions, orchestration

- `Recipe`, `Step`, `Assertion`, `VerifierConfig` — the recipe model, loaded
  from JSON or YAML, with a Draft 2020-12 schema and structured validation.
- `steps` — the seven built-in step actions and their dispatch.
- `assertions` — the eight built-in assertions.
- `planner` / `runner` / `execution` — recipe × renderer planning and execution
  against a `terminal` session.
- `store` / `cache` — canonical artifact storage with a path-traversal guard,
  and a content-addressed run cache.
- `pyregex` / `pyrepr` / `pypath` / `pyschema` — the compatibility shims that
  keep this port's behaviour close to the Python oracle's.

### `terminal` — sessions and screen

- `PtySession` — a child process on a real pseudo-terminal via `portable-pty`,
  implementing the `Session` interface the rest of TermProof runs against.
- `TerminalScreen` — a `vt100` cell grid that interprets escape sequences
  rather than stripping them.
- `CastRecorder` / `replay_cast` — asciicast v2 recording and replay.
- `wait_for_idle` / `IdleTracker` — output-quiescence detection.
- `SessionBackend` implementations: `PtySessionBackend` (the default),
  `PluginSessionBackend`, and `InMemorySession` for tests.

### `evidence` — screenshots, video, reports

- `render_svg` / `render_by_extension` — plain screen text to an SVG, in
  default colours.
- `ScreenshotRenderer` — an attributed grid to a PNG, with the colours the
  terminal actually showed. It and `render_svg` draw through the same
  `screen_svg`, so an SVG still looks the same whichever produced it; pick on
  what you have, not on how you want it to look.
- `render_png` — plain screen text to a PNG with no external tool. It bundles
  no font, so it paints a block per occupied cell rather than glyphs: it shares
  the canvas, grid and palette with `render_svg` and nothing else. A PNG that
  looks like the SVG is a rasterised one, which is `ScreenshotRenderer`.
  `evidence::render`'s module docs have the table.
- `generate_markdown` — a human report for a run. Its machine counterpart is
  `junit::generate_junit`, behind the `junit` feature.
- `apply_visual_diff` — compare a screenshot against a stored baseline, or
  refresh the baseline.
- `render_mp4` — video via external `agg` and `ffmpeg` binaries, resolved at
  run time and failing with a named diagnostic when absent.

### Library surface no TermProof command uses yet

Fourteen tested APIs with no caller in the CLI. Useful if you are building on
the library; not evidence that a `termproof run` does any of it.
[`docs/status-and-parity.md`](../../docs/status-and-parity.md)
lists the same fourteen.

- `parity` — compares two runs and reports where they disagree.
- `before_after` — reports which outcomes flipped between two runs.
- `selection` — maps a changeset onto the recipes it affects, via `ci_paths`.
- `run_config` — a whole run described by one file.
- `vocabulary` — a configurable failure detector.
- `build_info` — provenance for the binary under test, so a result can be
  traced back to an exact artifact.
- `terminal::attributed` — a per-cell screen carrying foreground, background,
  bold, dim, italic, underline, strikethrough, reverse and display width, with
  an SVG renderer. A run writes no image at all today — `raw_output.txt`,
  `screen.txt` and a cast if one was recorded — so nothing in the CLI reaches
  this or the text path.
- `terminal::tmux` — a `Session` that runs the program in a tmux pane and reads
  the grid back with `capture-pane`. A disagreement between it and the `vt100`
  path is an emulation gap made visible.
- `terminal::proc` — child processes with a deadline.
- `terminal::driver` — `SessionDriver`, a scenario-facing wrapper over
  `Box<dyn Session>`: implement `Session` to write a backend, use
  `SessionDriver` to write a scenario. Its tests are the integration suite
  `tests/session_driver.rs` rather than a unit module.
- `evidence::screenshot` and `evidence::cast_video` — stills and video frames
  rendered through one renderer rather than two unrelated ones.
- `evidence::dedup` — skips re-rendering a screen identical to the step before
  it.
- `evidence::uploader` — a publishing seam with a fallback chain that records
  which store it fell back from.

## Measured agreement, not parity

The step and assertion layers are measured against corpora recorded from the
Python implementation. On those corpora the two runtimes reach 82/115 full
agreement on steps and 124/147 on assertions. That is a layer-level number, not
a product-level one — screen fidelity and whole-recipe execution are outside
it. `harness/README.md` in the repository is the authority on the counts and
the divergences; [`docs/status-and-parity.md`](../../docs/status-and-parity.md)
carries the full inventory of what a run still cannot do.

`load_canonical_schema` finds nothing in this repository: the canonical recipe
schema and the example corpus stay with the Python repository on purpose, as
the contract both implementations answer to.

What this crate does pin is its *own* generated schema, to a checked-in
snapshot (`tests/snapshots/recipe_schema_v1.json`, guarded by
`tests/schema_snapshot.rs`). The guard compares parsed JSON trees, so object
key order is ignored but any structural drift in `generate_recipe_schema()`'s
output — keywords, numbers, array order, `$ref` targets — fails the test, and
re-blessing is a deliberate env-var flow (`TERM_PROOF_BLESS_SCHEMA=1`). It
catches accidental changes to this crate's schema; it does not establish
agreement with the canonical schema, which remains parity-gate work.

## Known gaps

- `terminal::DockerSessionBackend` is a stub.
- `terminal::InMemorySession` encodes test-passing rather than PTY semantics:
  `wait_for_text` answers from fixed content and ignores its deadline, and
  `wait_for_idle` always returns true.
- Two `press` key mappings (`ctrl-[`, `ctrl-1`) are refused here that the
  Python oracle accepts.
- The CLI parses `--video`, `--diff` and `--update-baselines` but does not yet
  call into the corresponding `evidence` functions; they are reachable as a
  library API, not from a `termproof run`.

A recipe also cannot branch on what it observes, and that one is a decision
rather than a gap. A scenario that polls until something renders and acts only
if it did, dismisses an overlay that may or may not appear, or retries a racy
step, belongs in the consumer's own runner — driving a session through
`terminal::SessionDriver`, using `evidence`, and building a `result::RunResult`
itself so `parity`, `before_after` and the reporters still apply.
`docs/conditional-recipes.md` in the repository has the reasoning, a worked
example against the driver, and the reversal conditions.

## Package contents

The two differential tests (`tests/differential_steps.rs`,
`tests/differential_assertions.rs`) are excluded from the published package —
they replay a corpus that lives at the repository root and is not shipped.

Everything else in `tests/` ships, including the schema snapshot
(`tests/snapshots/recipe_schema_v1.json`) and its guard
(`tests/schema_snapshot.rs`), so a consumer that runs the published crate's
tests gets the same drift check this repository runs. `cargo package --list`
is the proof. The differential tests are run from a repository checkout.

## Licence

MIT — see [LICENSE](LICENSE).
