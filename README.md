# termproof-rust — archived, consolidated into md-mt/termproof

**This repository is read-only. Development continues at
https://github.com/md-mt/termproof, where the Rust implementation lives under `rust/`.**

## What happened

On 2026-08-16 this repository was merged into `md-mt/termproof` so that the Python and Rust
implementations, the specification and the shared conformance corpus live in one place. The Rust
crate is unaffected: it is still published to crates.io as `termproof`, and 0.3.4 onward is
released from the consolidated repository.

## Where things went

| Here | There |
|---|---|
| `crates/` | `rust/crates/` |
| `specs/` | `spec/` |
| `harness/` | `conformance/` |
| `docker/` | `rust/docker/` |
| `.github/workflows/*` | `.github/workflows/rust-*` |

Release tags are now prefixed: `rs-v*` for the crate, `py-v*` for the Python package.

## Why this repository is kept rather than deleted

Two reasons, and the second matters more than it looks.

1. **Published metadata points here.** Releases up to 0.3.3 on crates.io were published from this
   repository, and its tags are referenced from that history.
2. **This is the only place the pre-consolidation Rust commit history exists.** The consolidation
   pull request was squash-merged, so `md-mt/termproof` carries the Rust *content* but not the Rust
   *lineage* — `git log --follow` on a file under `rust/` there will not reach back past the
   consolidation. The commits from the workspace baseline through `bb8dc3af` are preserved here and
   nowhere else.

If you need the history of a Rust file as it was written, clone this repository.

## Open work

The two issues that were open here moved to the consolidated repository:

- Oracle expectations depend on the machine that regenerates them → md-mt/termproof#172
- Repository maintenance: audited metadata and main ruleset → md-mt/termproof#173
