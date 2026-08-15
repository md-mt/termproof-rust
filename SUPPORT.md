# Support

## What this project is

`termproof-rust` is the **Rust reimplementation** of
[TermProof](https://github.com/md-mt/termproof). The Python implementation at
[`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
and the behavioural oracle; this port is **in progress** and is **not at
parity** with it.

**Before asking for help, read the
[maturity section of the README](README.md#maturity--read-this-before-using-it).**
Most "why doesn't this work the way I expect" questions about the port are
answered there — including the list of what a run still cannot do.

## Where to ask

This repository does not have GitHub Discussions enabled. Questions belong in
the issue tracker:

- **Open an issue** (blank issues are enabled) and the maintainers will add
  the `question` label.
- For a suspected defect, use the
  [bug report form](.github/ISSUE_TEMPLATE/bug_report.yml); for behaviour
  that differs from the Python implementation, use the
  [parity gap form](.github/ISSUE_TEMPLATE/parity_gap.yml).

## What is in scope

- Building, testing and using the Rust crates in this repository.
- The differential harness under `harness/` and the counts it reports.
- The engineering and release documentation (`docs/`).

## What is out of scope

- **Questions about TermProof the product, or about the Python
  implementation.** Those belong in
  [`md-mt/termproof`](https://github.com/md-mt/termproof/issues). This port is
  not the authority on TermProof's behaviour.
- **Parity claims.** This port is not a drop-in replacement; treat any claim
  that it behaves like the Python implementation as unverified.

## What support you can expect

This is a small, best-effort, pre-1.0 project. The maintainers answer issues
as time allows — there is **no SLA** and no guaranteed response time. Issues
that are clearly answered by the README, the docs, or a search of the tracker
may be closed with a pointer rather than a full reply.

## Documentation

- [`docs/rust-reimplementation-spec.md`](docs/rust-reimplementation-spec.md) —
  design rationale, compatibility contract and parity gates.
- [`docs/engineering-baseline.md`](docs/engineering-baseline.md) — workspace
  engineering policy.
- [`docs/publishing.md`](docs/publishing.md) — release mechanics.
- [`harness/README.md`](harness/README.md) — the differential harness.
