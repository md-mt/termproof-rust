# termproof-terminal

Pseudo-terminal and process sessions, vt100 screen state and asciicast
recording — the terminal layer of
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

## Known gaps

- `DockerSessionBackend` is a stub.
- `InMemorySession` encodes test-passing rather than PTY semantics:
  `wait_for_text` answers from fixed content and ignores its deadline, and
  `wait_for_idle` always returns true.
- Two `press` key mappings (`ctrl-[`, `ctrl-1`) are refused here that the
  Python oracle accepts.

## Licence

MIT — see [LICENSE](LICENSE).
