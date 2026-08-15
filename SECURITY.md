# Security Policy

## Status

This repository is an **in-progress, pre-1.0 Rust reimplementation** of
TermProof. Read the [maturity section of the README](README.md#maturity--read-this-before-using-it)
before depending on anything here: the port is **not at parity** with the
Python implementation, and no parity gate exists.

- **Nothing has been published to crates.io yet.** There is no published
  crate to patch and no registry consumer to notify.
- The only release artifacts are the GitHub Releases cut by
  `.github/workflows/auto-release.yml`, carrying binaries built by
  `.github/workflows/release-rust.yml` (which, per its own header, has **never
  run end to end** in this repository — treat those binaries as unverified
  until the release identity review it calls for is done).
- CI runs on GitHub-hosted runners; the workflows live in `.github/workflows/`.

## Reporting a vulnerability

Please report security issues privately rather than opening a public issue, so
a fix can land before the details are widely known.

1. **If private vulnerability reporting is enabled for this repository**, use
   it — on the repository page, *Security* → *Report a vulnerability*.
2. **Otherwise**, open an issue with `[SECURITY]` as the title prefix. The
   repository is small and every issue is read; if you prefer not to use the
   public tracker at all, contact the maintainer through their GitHub profile
   (the account that owns this repository).

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
- Because nothing is published to a registry, the "supported versions" set is
  effectively **the latest `main`**. Do not assume an older tag is patched.

## Scope

Everything in this repository is in scope: the workspace crates under
`crates/`, the CI workflows and scripts under `.github/`, the harness under
`harness/`, and the documentation. The Python implementation at
[`md-mt/termproof`](https://github.com/md-mt/termproof) is a separate
repository with its own security policy — a vulnerability there should be
reported there, not here.
