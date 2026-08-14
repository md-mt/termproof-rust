# Publishing to crates.io

Nothing here has been published yet. The first release reserves the name it
uses, and **crates.io never releases a name once it is taken, even after a
yank** — so read [What publishes, and what does
not](#what-publishes-and-what-does-not) before the first release rather than
after it.

## What publishes, and what does not

One crate is in scope:

| Crate | Publishes | Why |
|---|---|---|
| `termproof` | yes | the whole library: recipe model, steps, assertions, orchestration, terminal sessions, evidence pipeline |
| `termproof-cli` | **held** | `publish = false` |
| `termproof-plugin-protocol` | **held** | `publish = false` |

`termproof-plugin-protocol` is held because it is a leaf nothing depends on,
serving a plugin ecosystem that does not exist yet, and its shape will move as
the port approaches parity. `termproof-cli` is held for now as a deliberate
choice about what the first release commits to.

Both keep complete metadata. Lifting `publish = false` is the only change
needed to publish either of them — the release automation derives its set from
that field, so nothing else has to be edited.

`termproof` was merged from `termproof-core`, `termproof-terminal` and
`termproof-evidence` while all three were still unpublished, so none of those
names was ever reserved and no consumer was ever pointed at one. Nothing in the
tooling below hardcoded the old list, which is why the merge needed no edit
here beyond this prose.

## The dependency graph

Verified from the manifests with `cargo metadata`, not assumed:

```
termproof                          (no internal dependencies)
        │
        ├── termproof-cli              → termproof   [held]
        │
        └── termproof-plugin-protocol  → termproof   [held]
```

Both held crates are leaves, and the one publishable crate has no internal
dependencies at all, so the order below has nothing to sort.

## Publish order

**`termproof`, and nothing else.**

Do not copy that anywhere. `.github/scripts/publish-plan.py` derives it
from `cargo metadata` — every workspace member whose `publish` is not false,
topologically sorted over its internal dependencies — and prints:

```console
$ .github/scripts/publish-plan.py
{"version": "0.2.1", "order": ["termproof"], "held": ["termproof-cli", "termproof-plugin-protocol"]}
```

The derivation agrees with what the manifests say. It also refuses two states
that would produce a broken release:

- a publishable crate that depends on a held one, which could never resolve on
  the registry;
- workspace members that disagree on version, which would make the release tag
  meaningless.

## How to publish

### Normal path: publish a GitHub release

`.github/workflows/publish-crates.yml` runs on `release: published`. It:

1. derives the publish set, order and version;
2. **refuses a tag that disagrees with the manifest** — see
   [Tag format](#tag-format);
3. runs the full gate — `cargo fmt --check`, `cargo clippy … -D warnings`,
   `cargo test --workspace`, `cargo build --workspace --release`;
4. publishes each crate in the derived order, waiting for the crates.io index
   to show each version before starting the next.

It authenticates with the `CARGO_REGISTRY_TOKEN` repository secret.

**A `workflow_dispatch` run is always a dry run.** There is no input to flip.
The only path that uploads is a published release, so the workflow cannot be
hand-triggered into publishing by mistake. Use dispatch to rehearse.

The workflow also runs on every pull request, as a dry run, so it is exercised
before a release depends on it.

### Retrying a partial release

Re-run the workflow from the release. The publish step is idempotent: it asks
the crates.io index what is already there and skips it, so a re-run finishes
whatever is left rather than aborting on `crate version already exists`. With
one crate in the set that is the difference between a clean no-op and a red
release; if the set ever grows again it is the difference between finishing a
partial release and being unable to.

This is why the workflow loops rather than calling `cargo publish --workspace`,
which orders and waits by itself but aborts when a version is already on the
registry — precisely the state a retry starts from.

### Manual fallback

If the workflow is unusable, publish by hand. There is one crate and it has no
internal dependencies, so there is no order to get wrong and nothing to wait
for between uploads:

```sh
.github/scripts/publish-plan.py            # confirm the set first
cargo publish -p termproof
```

Rehearse on a clean target directory:

```sh
CARGO_TARGET_DIR=$(mktemp -d) cargo publish --dry-run -p termproof
```

The clean target directory is not superstition. Cargo treats registry sources
as immutable and keys a package by name and version alone, so a second
rehearsal of the *same* version, after the source has changed, can reuse the
artefact built from the first one and fail on code that is no longer there —
typically as an unresolved import for a module that is plainly present. The
failure looks like a real defect and is not.

CI is not exposed to this — no workflow caches `~/.cargo` or `target/`, so
every run starts empty. Keep it that way, or the release gate inherits the
same trap.

## Tag format

**`v<version>` — `v0.2.1`, not `0.2.1`.** A tag in any other form fails the
release before anything is uploaded.

Both forms are common in the wild, so this is a choice rather than a rule.
`v`-prefixed wins because `.github/workflows/release-rust.yml` already triggers
on `v*.*.*` to build the binaries. Accepting a bare `0.2.1` as well would allow
a tag that publishes the crates but never builds the binaries — a release that
is half-done and looks complete. One format, and it is the one already in use.

The check is exact string equality against `v$VERSION`, where `$VERSION` comes
from `cargo metadata`. So `v0.2.1` against a workspace at `0.2.1` passes;
`0.2.1`, `V0.2.1`, `v0.2.1-rc1` and `v0.3.0` all fail with a message naming
both values.

## Version-bump rule

- **One version for the whole workspace.** All three crates inherit `version`
  from `[workspace.package]` and are released together at the same number. The
  plan script refuses to run if they ever disagree.
- Bump `version` in the root `Cargo.toml` **and** the `version` on each
  internal dependency in `[workspace.dependencies]` in the same commit. These
  must never drift: the `path` is what a local build uses and the `version` is
  what the published package resolves against, and cargo will not catch a stale
  `version` until publish time.
- Pre-1.0, treat a breaking change to any public API as a minor bump (`0.2.x` →
  `0.3.0`) and everything else as a patch bump.
- Run `cargo update -w` after the bump so `Cargo.lock` matches, and commit it.

## Before you cut a release

The workflow enforces the mechanical items; these are the ones it cannot.

- [ ] The version bump and `Cargo.lock` are committed and on `main`.
- [ ] The tag is `v<version>` and points at that commit.
- [ ] `cargo package --list -p <crate>` for each publishable crate — read it,
      do not skim it. Anything large, generated, or repository-only does not
      belong in a tarball.
- [ ] The maturity warning in the crate's README still describes the port
      accurately. It is what the crates.io front page carries; a stale one is
      a claim of parity that has not been earned.
- [ ] Nothing newly publishable was made publishable by accident — check the
      `held` list in the plan output.

## What is not shipped, and why

The published tarballs are deliberately smaller than the repository:

- **`harness/`** — the Python probe, the checked-in corpus and the recorded
  oracle expectations (~150 KB). It lives at the repository root, outside every
  crate directory, so it is never packaged. It is a measurement artefact for
  contributors, not something a consumer of the library needs.
- **`crates/termproof/tests/differential_steps.rs` and
  `differential_assertions.rs`** — excluded explicitly. They replay
  `harness/corpus/`, so without it they cannot run; shipping tests that cannot
  run is worse than not shipping them. Run them from a repository checkout.
- **`specs/`, `docs/`, `.github/`** — repository root, never packaged.

Everything else the crate needs at build time is inside its own directory, and
the dry run proves it: the package is verified by compiling it from its own
tarball.

## Licensing

Every crate declares `license = "MIT"`, inherited from `[workspace.package]`,
and carries its own copy of `LICENSE` in the tarball. Cargo does **not**
automatically include a workspace-root `LICENSE` in a member's package, so the
file is copied into each crate directory; keep the copies in sync with the root
`LICENSE` on any licence change.

## Relationship to the other workflows

- `.github/workflows/publish-crates.yml` — the only thing that uploads to a
  registry.
- `.github/workflows/release-rust.yml` — builds and attests the `termproof`
  binary for tagged releases. It does not publish to crates.io. Its own header
  notes it has never run in this repository.
- `.github/workflows/rust.yml` — fmt, clippy and tests on every pull request.
  It has no packaging step: the release workflow's pull-request dry run covers
  that, and duplicating it would mean two places to keep correct.
