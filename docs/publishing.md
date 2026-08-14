# Publishing to crates.io

This workspace publishes five crates. Nothing here has been published yet —
the first release reserves all five names, and **crates.io never releases a
name once it is taken, even after a yank**. Read
[Before you publish anything](#before-you-publish-anything) before the first
release rather than after it.

## The dependency graph

Verified from the manifests with `cargo metadata`, not assumed:

```
termproof-terminal          (no internal dependencies)
        │
        ├── termproof-core                      → termproof-terminal
        │        │
        │        ├── termproof-evidence         → termproof-core, termproof-terminal
        │        │        │
        │        │        └── termproof-cli     → termproof-core, termproof-evidence
        │        │
        │        └── termproof-plugin-protocol  → termproof-core, termproof-terminal
```

`termproof-plugin-protocol` is a leaf — nothing depends on it — but it is *not*
at the bottom of the graph: it depends on both `termproof-core` and
`termproof-terminal`, so it cannot go first.

## Publish order

Any topological order works. The canonical one:

1. `termproof-terminal`
2. `termproof-core`
3. `termproof-evidence`
4. `termproof-plugin-protocol`
5. `termproof-cli`

`termproof-evidence`, `termproof-plugin-protocol` and `termproof-cli` have one
ordering constraint between them: `termproof-cli` must follow
`termproof-evidence`. `termproof-plugin-protocol` may go anywhere after
`termproof-core`.

## How to publish

### Preferred: let cargo do the ordering

Cargo 1.90 and later publish a whole workspace in one command. It computes the
order itself, verifies each package against the tarballs of the ones before it,
and **waits for registry propagation between crates** — which is the step a
manual sequence gets wrong:

```sh
cargo publish --workspace
```

Always run it as a dry run first. A dry run must be clean, not `--allow-dirty`:

```sh
cargo publish --dry-run --workspace
```

### Fallback: one crate at a time

Only if the workspace publish is unusable. Publish in the order above and wait
for each crate to appear on the index before the next, or the next crate's
verification step will fail with `no matching package named ... found`:

```sh
cargo publish -p termproof-terminal
# wait until `cargo search termproof-terminal` reports 0.2.1 — usually seconds,
# occasionally a few minutes
cargo publish -p termproof-core
# ... and so on, in order
```

A single-crate `--dry-run` of anything but `termproof-terminal` fails today
with `no matching package named ... found`, because the dependency is not on
the registry yet. That is expected and is not a defect in the package. To
verify a dependent crate before the first release, use the workspace dry run —
it overlays the not-yet-published tarballs in a temporary local registry.

## Version-bump rule

- **One version for the whole workspace.** All five crates inherit
  `version` from `[workspace.package]`; they are released together at the same
  number. Do not version them independently — the internal dependency
  constraints assume lockstep.
- Bump `version` in the root `Cargo.toml` **and** the `version` on each
  internal dependency in `[workspace.dependencies]` in the same commit. These
  two must never drift: the `path` is what a local build uses and the `version`
  is what the published package resolves against, and cargo will not catch a
  stale `version` until publish time.
- Pre-1.0, treat a breaking change to any public API as a minor bump (`0.2.x` →
  `0.3.0`) and everything else as a patch bump.
- Run `cargo update -w` after the bump so `Cargo.lock` matches, and commit it.

## Before you publish anything

- [ ] `cargo fmt --check --all`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo publish --dry-run --workspace` — clean tree, no `--allow-dirty`
- [ ] `cargo package --list -p <crate>` for each crate — read it, do not skim
      it. Anything large, generated, or repository-only does not belong in a
      tarball.
- [ ] The working tree is clean and the release commit is on `main`.
- [ ] The maturity warning in each crate's README still describes the port
      accurately. Every crate's crates.io front page carries it; a stale one is
      a claim of parity that has not been earned.
- [ ] The version, the git tag and `Cargo.lock` agree.

## What is not shipped, and why

The published tarballs are deliberately smaller than the repository:

- **`harness/`** — the Python probe, the checked-in corpus and the recorded
  oracle expectations (~150 KB). It lives at the repository root, outside every
  crate directory, so it is never packaged. It is a measurement artefact for
  contributors, not something a consumer of the library needs.
- **`crates/termproof-core/tests/differential_steps.rs` and
  `differential_assertions.rs`** — excluded explicitly. They replay
  `harness/corpus/`, so without it they cannot run; shipping tests that cannot
  run is worse than not shipping them. Run them from a repository checkout.
- **`specs/`, `docs/`, `.github/`** — repository root, never packaged.

Everything else each crate needs at build time is inside its own directory, and
the workspace dry run proves it: each package is verified by compiling it from
its own tarball.

## Licensing

Every crate declares `license = "MIT"`, inherited from `[workspace.package]`,
and carries its own copy of `LICENSE` in the tarball. Cargo does **not**
automatically include a workspace-root `LICENSE` in a member's package, so the
file is copied into each crate directory; keep the copies in sync with the root
`LICENSE` on any licence change.

## Relationship to the release workflow

`.github/workflows/release-rust.yml` builds and attests the `termproof` binary
for tagged releases. It does **not** publish to crates.io, and this document
does not ask it to. Registry publishing stays a deliberate manual step until
the port has a parity gate and a release identity review — automating a
one-way-door action that nobody has exercised is worse than running it by hand.

`.github/workflows/rust.yml` runs `cargo publish --dry-run --workspace` on
every pull request, so metadata regressions — a missing `version` on a path
dependency, a file that stopped being packaged — surface before a release
rather than during one.
