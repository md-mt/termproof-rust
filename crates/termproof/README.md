# termproof

Evidence-first verification for TUI and terminal applications — the library
half of [TermProof](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
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
  Markdown and JUnit reports, visual baselines, diff and upload.

## Features

Both are on by default, so `termproof = "0.3"` is the whole crate — the shape
that has always been published.

| Feature | Enables | Costs |
|---|---|---|
| `evidence` | the `evidence` module | `image`, `quick-junit`, `avt` |
| `json-schema` | `validation`, `pyschema`, and the `json_schema` built-in assertion | `jsonschema` |

Turning both off takes the crate from 180 transitive dependencies to 72:

```toml
termproof = { version = "0.3", default-features = false }
```

That build still has the whole session layer and the other seven built-in
assertions; what it loses is the evidence pipeline and JSON Schema validation.
Schema *generation* (`schema`, via `schemars`) is unconditional — it is four
crates, against `jsonschema`'s 87.

There is no `terminal` feature. The crate root is built on `terminal` and
nothing in `terminal` depends on the root, so a build without it would not be
this crate; it would also save nothing, since every build drives a terminal.

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
- `generate_markdown` / `generate_junit` — human and machine reports for a run.
- `apply_visual_diff` — compare a screenshot against a stored baseline, or
  refresh the baseline.
- `render_mp4` — video via external `agg` and `ffmpeg` binaries, resolved at
  run time and failing with a named diagnostic when absent.

### Library surface no TermProof command uses yet

Fourteen tested APIs with no caller in the CLI. Useful if you are building on
the library; not evidence that a `termproof run` does any of it. The workspace
README lists the same fourteen.

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
the divergences.

`load_canonical_schema` finds nothing in this repository: the canonical recipe
schema and the example corpus stay with the Python repository on purpose, as
the contract both implementations answer to.

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
Run them from a repository checkout.

## Licence

MIT — see [LICENSE](LICENSE).
