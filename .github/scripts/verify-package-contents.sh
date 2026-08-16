#!/usr/bin/env bash
# Verify the termproof package tarball contains exactly what the publishing
# contract (docs/publishing.md) promises — and nothing it does not.
#
# Run from a checkout after `cargo package -p termproof`. The --list output
# is the authoritative manifest of the tarball, so this script checks that
# list rather than the files on disk.
#
# Required: the snapshot test and its fixture ship (issue #33 added the
# snapshot on the promise that consumers could run it), and the differential
# tests — which replay harness/corpus/ and cannot run without it — do not.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

LIST="$(cargo package -p termproof --list --allow-dirty)"

# Present: the pieces a consumer needs and the tests that can run without the
# repository around them.
for required in \
  Cargo.toml \
  Cargo.toml.orig \
  LICENSE \
  README.md \
  src/lib.rs \
  tests/schema_snapshot.rs \
  tests/snapshots/recipe_schema_v1.json
do
  if ! grep -qx "$required" <<<"$LIST"; then
    echo "::error::termproof tarball is missing $required" >&2
    exit 1
  fi
done

# Absent: repository-only artifacts that must not reach consumers. The
# differential tests replay harness/corpus/, which lives at the repository
# root and is not shipped; a tarball that carries them cannot run them.
for forbidden in \
  tests/differential_steps.rs \
  tests/differential_assertions.rs \
  harness/
do
  if grep -q "^${forbidden}" <<<"$LIST"; then
    echo "::error::termproof tarball must not contain $forbidden" >&2
    exit 1
  fi
done

echo "termproof tarball contents verified ($(wc -l <<<"$LIST") files)"
