#!/usr/bin/env python3
"""Write a new version into the root manifest, then prove it took.

    .github/scripts/version-bump.py 0.2.2
    .github/scripts/version-bump.py 0.2.2 --check   # report, change nothing

Two places in the root `Cargo.toml` carry the version and must move together,
per the version-bump rule in docs/publishing.md:

  - `[workspace.package] version`, which every member inherits;
  - the `version` on each internal dependency in `[workspace.dependencies]`,
    which is what the *published* package resolves against. The `path` beside
    it is what a local build uses, so a stale `version` here compiles fine
    locally and fails at publish time, which is the worst moment to find it.

Internal dependencies are identified by the presence of `path = `, not by
name. No crate is named in this file, so merging several crates into one, or
adding one, needs no edit here.

Editing is a line-level substitution rather than a TOML round-trip, so
comments, ordering and formatting survive byte-for-byte. The safety net is not
the parser, it is the verification at the end: `cargo metadata` must report
every workspace member at the new version, or this exits non-zero having told
you which ones did not move.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

SEMVER = re.compile(r"^\d+\.\d+\.\d+$")
SECTION = re.compile(r"^\s*\[([^\]]+)\]\s*$")

# `version = "0.2.1"` as a bare key, anywhere on the line.
BARE_VERSION = re.compile(r'(?P<lead>^\s*version\s*=\s*")(?P<ver>[^"]+)(?P<tail>")')

# `version = "0.2.1"` inside an inline table, e.g.
# `foo = { path = "crates/foo", version = "0.2.1" }`.
INLINE_VERSION = re.compile(r'(?P<lead>\bversion\s*=\s*")(?P<ver>[^"]+)(?P<tail>")')


def cargo_metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def rewrite(text, old, new):
    """Return (new_text, [description of each edit])."""
    out = []
    edits = []
    section = None
    for lineno, line in enumerate(text.splitlines(keepends=True), start=1):
        header = SECTION.match(line)
        if header:
            section = header.group(1).strip()
            out.append(line)
            continue

        if section == "workspace.package":
            match = BARE_VERSION.match(line)
            if match and match.group("ver") == old:
                line = BARE_VERSION.sub(rf"\g<lead>{new}\g<tail>", line, count=1)
                edits.append(f"line {lineno}: [workspace.package] version -> {new}")

        elif section == "workspace.dependencies" and "path" in line:
            match = INLINE_VERSION.search(line)
            if match and match.group("ver") == old:
                key = line.split("=", 1)[0].strip()
                line = INLINE_VERSION.sub(rf"\g<lead>{new}\g<tail>", line, count=1)
                edits.append(f"line {lineno}: [workspace.dependencies] {key} version -> {new}")

        out.append(line)
    return "".join(out), edits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="the new version, e.g. 0.2.2")
    parser.add_argument("--check", action="store_true", help="report edits without writing")
    args = parser.parse_args()

    new = args.version
    if not SEMVER.match(new):
        sys.exit(f"error: '{new}' is not a bare x.y.z version")

    meta = cargo_metadata()
    root = Path(meta["workspace_root"])
    manifest = root / "Cargo.toml"

    old = sorted({pkg["version"] for pkg in meta["packages"]})
    if len(old) != 1:
        detail = ", ".join(f"{p['name']} {p['version']}" for p in meta["packages"])
        sys.exit(f"error: workspace members disagree on version: {detail}")
    old = old[0]

    if old == new:
        print(f"already at {new}; nothing to do")
        return

    text = manifest.read_text()
    updated, edits = rewrite(text, old, new)
    if not edits:
        sys.exit(f"error: found nothing to bump from {old} to {new} in {manifest}")

    for edit in edits:
        print(edit)

    if args.check:
        print("--check: no files written")
        return

    manifest.write_text(updated)

    # `cargo update -w` refreshes only the workspace members' own entries in
    # Cargo.lock, leaving every third-party pin exactly where it was. Without
    # it the lockfile still claims the old version and the release commit is
    # internally inconsistent.
    subprocess.run(["cargo", "update", "-w"], check=True)

    after = cargo_metadata()
    stale = sorted(p["name"] for p in after["packages"] if p["version"] != new)
    if stale:
        sys.exit(
            f"error: after the bump these members are still not at {new}: {', '.join(stale)}. "
            "They probably do not inherit version from [workspace.package]."
        )
    print(f"workspace is at {new}: {', '.join(sorted(p['name'] for p in after['packages']))}")


if __name__ == "__main__":
    main()
