# Changelog

All notable changes to **termproof-rust** are documented in this file.

This repository is the **Rust reimplementation** of
[TermProof](https://github.com/md-mt/termproof). The Python implementation at
[`md-mt/termproof`](https://github.com/md-mt/termproof) is the shipped product
and the behavioural oracle; this port is **in progress** and is not at parity
with it. Read the
[maturity section of the README](https://github.com/md-mt/termproof-rust#maturity--read-this-before-using-it)
before relying on anything here.

- Releases are cut by `.github/workflows/auto-release.yml` and published as
  GitHub Releases; the `termproof` crate is published to crates.io from those
  releases by `.github/workflows/publish-crates.yml`. Those workflows,
  `docs/publishing.md` and the git tags are the source of truth for release
  mechanics. This file is the curated, hand-written view, kept in sync with
  the tags.
- **`termproof` is published on crates.io through `0.3.2`** (`0.2.1`, `0.3.0`,
  `0.3.1`, `0.3.2`, all unyanked). The other workspace crates —
  `termproof-cli` and `termproof-plugin-protocol` — are **held back**
  (`publish = false`) and are not on the registry; the pre-merge split names
  (`termproof-core`, `termproof-terminal`, `termproof-evidence`) were merged
  into `termproof` before any of them was published, so none was ever
  reserved.
- Version numbers apply to the whole workspace (all crates share one version
  from `[workspace.package]`), per the version-bump rule in
  [`docs/publishing.md`](docs/publishing.md).
- The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
  and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
  as modified by the pre-1.0 rule in `docs/publishing.md` (under `0.x`, a
  breaking change bumps the minor digit).

## [Unreleased]

Nothing yet. Add entries here, under the heading of the release they will land
in, as part of the PR that introduces the change.

## [0.3.2] - 2026-08-15

### Changed

- **cargo:** JUnit output gets its own feature, so a consumer who only wants
  JUnit stops paying for the evidence renderers (`#36`).
- **cargo:** the portable-pty and unicode-width floors are documented from
  outside, so the reason each floor sits where it does is legible without
  reading the code (`#35`, `#37`).
- **evidence:** the JUnit writer moves to its own module (`167cc96`).

## [0.3.1] - 2026-08-14

### Added

- **cargo:** `schema` — schemars moves behind a default-on feature, so a
  consumer that does not need schema generation stops compiling it (`#28`).
- **cargo:** default-on `evidence` and `json-schema` features, so a consumer
  compiles only what it uses (`#31`).
- **terminal:** `Session::cwd()`, reporting where the child process actually
  went (`#30`).

### Changed

- **cargo:** every version requirement is now a tested floor — CI pins each
  widened requirement to its floor and runs the suite against it, so a floor
  that stops being true fails CI rather than rotting (`#32`).
- **steps:** type inference names the regex `Captures` type instead of a
  concrete version of it (`d23c727`).

## [0.3.0] - 2026-08-14

### Added

- **evidence:** `EvidenceCollector`, an ordered step model beside `RunResult`
  (`#26`).
- **terminal:** `SessionDriver`, a scenario-facing layer over `Session`
  (`#23`).
- **result:** the `RunResult` payload is versioned, with an absent version its
  own rule (`#22`).

### Changed

- **evidence:** one SVG renderer behind both stills and video — proposal for
  `#19`, and the change that bumped the minor digit under the pre-1.0 rule
  (`#25`).
- **terminal:** `dim` is carried through the vt100 path (vt100 `0.15` →
  `0.16`) (`#21`).
- **docs:** conditional recipes are declined, and the docs say what a consumer
  with a branching scenario uses instead (`#24`).

## [0.2.1] - 2026-08-13

First release of the split repository, covering everything from the subtree
seed through the release automation (`#14`). Highlights:

### Added

- **release:** weekly auto-release that only fires on real change, and a
  complete GitHub Release (`#14`).
- **cargo:** every crate made publishable to crates.io, with the publish set
  and order derived from `cargo metadata` rather than a maintained list (`#11`).
- **core:** the eight built-in assertions, measured against the Python oracle
  (`#10`).
- **execution:** `PtySession` is a `Session`, and `termproof run` runs recipes
  against a real child (`#9`).
- **terminal:** the terminal layer is real — children run on a real
  pseudo-terminal via `portable-pty`, and the screen is a `vt100` cell grid
  that interprets escapes instead of stripping them (`#6`).
- **core:** assertions get the screen captured after each step (`#5`).
- **spec:** Spec Kit adopted and the core verification semantics specified
  (`#4`).
- **docs:** the reimplementation spec is brought across from `md-mt/termproof`.

### Changed

- **refactor:** `termproof-core`, `termproof-terminal` and `termproof-evidence`
  merge into one crate named `termproof` before any of them is published
  (`#13`).
- **core:** the five step-layer defects fixed, each with a test that failed
  first (`#7`).
- **docs:** the README's maturity section brought back to what the code does
  (`#8`).

[Unreleased]: https://github.com/md-mt/termproof-rust/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/md-mt/termproof-rust/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/md-mt/termproof-rust/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/md-mt/termproof-rust/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/md-mt/termproof-rust/releases/tag/v0.2.1
