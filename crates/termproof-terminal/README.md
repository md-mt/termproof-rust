# termproof-terminal

Pseudo-terminal, tmux and process sessions, plain and attributed screen state,
and asciicast recording — the terminal layer of
[TermProof](https://github.com/md-mt/termproof-rust).

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> before depending on this crate.

## What it provides

- `PtySession` — a child process on a real pseudo-terminal via `portable-pty`,
  implementing the `Session` interface the rest of TermProof runs against.
- `TerminalScreen` — a `vt100` cell grid that interprets escape sequences
  rather than stripping them.
- `CastRecorder` / `replay_cast` — asciicast v2 recording and replay.
- `wait_for_idle` / `IdleTracker` — output-quiescence detection.
- `SessionBackend` implementations: `PtySessionBackend` (the default),
  `PluginSessionBackend`, and `InMemorySession` for tests.

### Library surface no TermProof command uses yet

These are tested APIs with no caller in the CLI. Useful if you are building on
the library; not evidence that a `termproof run` does any of it.

- `attributed` — a per-cell screen carrying foreground, background, bold, dim,
  italic, underline, strikethrough, reverse and display width, with an SVG
  renderer. The screenshots a run writes today still come from the
  single-colour text path.
- `tmux` — a `Session` that runs the program in a tmux pane and reads the grid
  back with `capture-pane`. A disagreement between it and the `vt100` path is
  an emulation gap made visible.
- `proc` — child processes with a deadline.

## Known gaps

- `DockerSessionBackend` is a stub.
- `InMemorySession` encodes test-passing rather than PTY semantics:
  `wait_for_text` answers from fixed content and ignores its deadline, and
  `wait_for_idle` always returns true.
- Two `press` key mappings (`ctrl-[`, `ctrl-1`) are refused here that the
  Python oracle accepts.

## Licence

MIT — see [LICENSE](LICENSE).
