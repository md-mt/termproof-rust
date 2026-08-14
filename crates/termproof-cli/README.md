# termproof-cli

The `termproof` command line binary — evidence-first verification for TUI and
terminal applications. Part of
[termproof-rust](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> before depending on this binary.

## Install

**This crate is not published to crates.io.** Build it from a checkout:

```sh
cargo build --release -p termproof-cli
```

The binary is named `termproof`. Prebuilt binaries for tagged releases are
attached to the [GitHub releases](https://github.com/md-mt/termproof-rust/releases).

## Use

```sh
termproof run <path>...
```

`run` discovers recipe files under the paths given, loads each one, plans
recipe × renderer, runs the steps against a real child on a pseudo-terminal,
and writes `result.json`, `report.md`, `raw_output.txt`, `screen.txt` and an
asciicast per run, plus `latest-report.md` — and a JUnit file when
`--xml-path` is given.

## What a run still cannot do

- **Only `execution: scripted` on a pty runs.** A recipe whose `command.pty` is
  false, or whose `execution` is anything else, is refused with a diagnostic
  naming the reason and a non-zero exit.
- **`--video`, `--diff`, `--update-baselines` and `--skip-unchanged` are parsed
  and ignored**, with a warning on stderr rather than silence.
- **Failures are not contained.** Turning recipe, step, plugin, process and PTY
  failures into structured results is not done yet.

## Licence

MIT — see [LICENSE](LICENSE).
