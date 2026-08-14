#!/usr/bin/env python3
"""Derive the crates.io publish plan from `cargo metadata`.

Emits JSON on stdout:

    {"version": "0.2.1", "order": ["termproof-terminal", "termproof-core", ...]}

`order` is every workspace member that is publishable to crates.io, sorted so
that each crate's internal dependencies come before it. `version` is the single
version the whole workspace shares.

Nothing here is hardcoded, deliberately. The publish set is a policy decision
already recorded in the manifests (`publish = false`), and the order follows
from the dependency graph. A list written down in a workflow file is a list
that goes stale the first time a crate is added, held back or merged into
another — and the failure mode is a half-published release. See
docs/publishing.md.
"""

import json
import subprocess
import sys


def load_members():
    """Workspace members only — `--no-deps` excludes the dependency graph."""
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)["packages"]


def publishable(pkg):
    """True when cargo would let this package go to crates.io.

    `publish` is null when unrestricted, `[]` for `publish = false`, and a list
    of registry names when restricted to specific registries.
    """
    allowed = pkg.get("publish")
    if allowed is None:
        return True
    return "crates-io" in allowed


def workspace_version(members):
    """The one version every member shares.

    All five crates inherit `version` from `[workspace.package]` and are
    released in lockstep. If that ever stops being true the release tag can no
    longer identify what was published, so this refuses to guess.
    """
    versions = sorted({pkg["version"] for pkg in members})
    if len(versions) != 1:
        detail = ", ".join(
            f"{pkg['name']} {pkg['version']}" for pkg in sorted(members, key=lambda p: p["name"])
        )
        sys.exit(f"error: workspace members disagree on version: {detail}")
    return versions[0]


def internal_edges(members, publish_set):
    """Map each publishable crate to the publishable crates it depends on.

    Dev-dependencies are left out: they do not constrain publish order, and an
    A-dev-depends-on-B / B-depends-on-A pair is legal in cargo but would look
    like a cycle here.
    """
    member_names = {pkg["name"] for pkg in members}
    edges = {}
    for pkg in members:
        if pkg["name"] not in publish_set:
            continue
        deps = set()
        for dep in pkg["dependencies"]:
            if dep.get("kind") == "dev" or dep["name"] not in member_names:
                continue
            if dep["name"] not in publish_set:
                sys.exit(
                    f"error: {pkg['name']} is publishable but depends on "
                    f"{dep['name']}, which is not. A published crate cannot "
                    f"resolve an unpublished path dependency — either publish "
                    f"{dep['name']} or hold {pkg['name']} back too."
                )
            deps.add(dep["name"])
        edges[pkg["name"]] = deps
    return edges


def toposort(edges):
    """Kahn's algorithm, ties broken by name so the order is reproducible."""
    remaining = {name: set(deps) for name, deps in edges.items()}
    order = []
    while remaining:
        ready = sorted(name for name, deps in remaining.items() if not deps)
        if not ready:
            sys.exit(f"error: dependency cycle among {sorted(remaining)}")
        for name in ready:
            order.append(name)
            del remaining[name]
        for deps in remaining.values():
            deps.difference_update(ready)
    return order


def main():
    members = load_members()
    publish_set = {pkg["name"] for pkg in members if publishable(pkg)}
    plan = {
        "version": workspace_version(members),
        "order": toposort(internal_edges(members, publish_set)),
        "held": sorted(pkg["name"] for pkg in members if pkg["name"] not in publish_set),
    }
    json.dump(plan, sys.stdout)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
