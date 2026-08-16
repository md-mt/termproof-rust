#!/usr/bin/env bash
# Smoke-test a release archive before it is uploaded: checksum, extraction,
# and `termproof --version` against the workspace version.
#
# Usage: verify-release-archive.sh <archive> [expected-version]
#
# The archive and its `.sha256` must live in the same directory. The checksum
# file is written from inside `dist/` (see release-rust.yml) so the recorded
# name is the archive alone; `shasum -c` is therefore run from that directory.
#
# With an expected version, the extracted binary's `--version` output must
# match `termproof <version>` exactly. This is what turns "the archive
# extracts" into "the archive extracts into the binary the release claims".
set -euo pipefail

archive="$1"
expected_version="${2:-}"

if [[ ! -f "$archive" ]]; then
  echo "::error::archive not found: $archive" >&2
  exit 1
fi

dir="$(cd "$(dirname "$archive")" && pwd)"
base="$(basename "$archive")"
checksum="$dir/$base.sha256"

if [[ ! -f "$checksum" ]]; then
  echo "::error::checksum file not found: $checksum" >&2
  exit 1
fi

# 1. Checksum — the .sha256 records the archive alone, so verify from the
#    same directory.
(cd "$dir" && shasum -a 256 -c "$base.sha256")

# 2. Extract into a scratch directory.
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
tar -xzf "$archive" -C "$scratch"

# 3. The archive must contain the binary at its root.
binary="$scratch/termproof"
if [[ ! -x "$binary" ]]; then
  echo "::error::$archive does not contain an executable 'termproof' at its root" >&2
  exit 1
fi

# 4. Run it. With an expected version, --version must name exactly that.
actual="$("$binary" --version)"
echo "extracted $base: termproof --version -> $actual"

if [[ -n "$expected_version" ]]; then
  if [[ "$actual" != "termproof $expected_version" ]]; then
    echo "::error::$base reports '$actual', expected 'termproof $expected_version'" >&2
    exit 1
  fi
  echo "version matches workspace version $expected_version"
fi
