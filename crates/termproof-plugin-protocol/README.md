# termproof-plugin-protocol

The versioned newline-delimited JSON protocol that
[TermProof](https://github.com/md-mt/termproof-rust) speaks to out-of-process
plugins, with client, host and conformance support.

> **Maturity: this port is in progress and is not at parity with the Python
> implementation.** The Python implementation at
> [`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
> and the behavioural oracle for TermProof; there is no parity gate for this
> port. Read
> [the maturity section of the workspace README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
> before depending on this crate.

**This crate is not published to crates.io.** It is a leaf that nothing else in
the workspace depends on, serving a plugin ecosystem that does not exist yet,
and its shape will move as the port approaches parity — so its name is not
being spent on the registry until the interface has settled. See
[`docs/publishing.md`](https://github.com/md-mt/termproof-rust/blob/main/docs/publishing.md).

## What it provides

- `protocol` — `Hello`, `Ready`, `Request`, `Response`, `Shutdown`,
  `Capability`, and the version, timeout and message-size constants that bound
  a session.
- `PluginClient` — the host side: spawn a plugin, negotiate a version, exchange
  requests.
- `run_plugin` / `PluginHandler` — the plugin side, with `EchoHandler` as a
  reference implementation.
- `conformance_roundtrip` — a minimal in-memory handshake and echo check a
  handler can be measured against.
- `PythonBridge` — talking to plugins written for the Python implementation.

## Licence

MIT — see [LICENSE](LICENSE).
