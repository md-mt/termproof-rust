# Security Policy

## Status

This repository is an **in-progress, pre-1.0 Rust reimplementation** of
TermProof. Read the [maturity section of the README](README.md#maturity--read-this-before-using-it)
before depending on anything here: the port is **not at parity** with the
Python implementation, and no parity gate exists.

- **`termproof` is published on crates.io through `0.3.2`** (`0.2.1`, `0.3.0`,
  `0.3.1`, `0.3.2`, all unyanked), so there are registry consumers to notify
  for a security fix. The other workspace crates — `termproof-cli` and
  `termproof-plugin-protocol` — are held back (`publish = false`) and are not
  on the registry.
- Release artifacts are the GitHub Releases cut by
  `.github/workflows/auto-release.yml`, carrying binaries built by
  `.github/workflows/release-rust.yml` — which has run successfully on every
  tag from `v0.2.1` through `v0.3.2` — and the `termproof` crate is published
  to crates.io by `.github/workflows/publish-crates.yml` on each published
  release.
- CI runs on GitHub-hosted runners; the workflows live in `.github/workflows/`.

## Reporting a vulnerability

Please report security issues through **GitHub Private Vulnerability
Reporting** — on the repository page, *Security* → *Report a vulnerability*.
Reports go only to the maintainers and stay private until a fix can land.

Do **not** disclose sensitive details — proof-of-concept code, exploit
write-ups, or affected-version specifics — in a public issue, pull request or
discussion. Public channels are not a private reporting path. If the private
reporting form is unavailable for any reason, contact the maintainer through
their GitHub profile (the account that owns this repository) instead of
opening a public issue with the details.

Please include:

- the affected commit, tag or branch (or say "latest `main`");
- a minimal recipe or test case that triggers the issue;
- your assessment of impact, if you have one.

## What to expect

This is a small, best-effort, pre-1.0 project; there is no security SLA.

- The maintainers aim to acknowledge reports within **7 days**.
- A fix lands through the normal PR flow and, when it is user-facing, an
  entry under `[Unreleased]` in `CHANGELOG.md`.
- The maintainers will coordinate disclosure with you. If you would prefer a
  specific embargo window, say so in the report.
- The "supported versions" set is effectively **the latest `main`** (and the
  latest published `termproof` crate). Do not assume an older tag or published
  version is patched.

## Scope

Everything in this repository is in scope: the workspace crates under
`crates/`, the CI workflows and scripts under `.github/`, the harness under
`harness/`, and the documentation. The Python implementation at
[`md-mt/termproof`](https://github.com/md-mt/termproof) is a separate
repository with its own security policy — a vulnerability there should be
reported there, not here.
