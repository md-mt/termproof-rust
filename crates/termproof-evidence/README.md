# termproof-evidence

Screenshot and video rendering, Markdown and JUnit reports, visual baselines,
diff and upload — the evidence pipeline of
[TermProof](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> before depending on this crate.

## What it provides

- `render_png` / `render_svg` / `render_by_extension` — screen state to an
  image.
- `generate_markdown` / `generate_junit` — human and machine reports for a run.
- `apply_visual_diff` — compare a screenshot against a stored baseline, or
  refresh the baseline.
- `render_mp4` — video via external `agg` and `ffmpeg` binaries, resolved at
  run time and failing with a named diagnostic when absent.

### Library surface no TermProof command uses yet

These are tested APIs with no caller in the CLI. Useful if you are building on
the library; not evidence that a `termproof run` does any of it.

- `screenshot` and `cast_video` — stills and video frames rendered through one
  renderer rather than two unrelated ones.
- `dedup` — skips re-rendering a screen identical to the step before it.
- `uploader` — a publishing seam with a fallback chain that records which store
  it fell back from.

## Known gaps

The CLI parses `--video`, `--diff` and `--update-baselines` but does not yet
call into the corresponding functions here; they are reachable as a library
API, not from a `termproof run`.

## Licence

MIT — see [LICENSE](LICENSE).
