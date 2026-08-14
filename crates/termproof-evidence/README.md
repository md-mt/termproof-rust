# termproof-evidence

Screenshot rendering, Markdown and JUnit reports, video, visual baselines and
diff — the evidence pipeline of
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

## Known gaps

The CLI parses `--video`, `--diff` and `--update-baselines` but does not yet
call into the corresponding functions here; they are reachable as a library
API, not from a `termproof run`.

## Licence

MIT — see [LICENSE](LICENSE).
