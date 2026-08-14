#!/usr/bin/env python3
"""Decide whether a release is warranted, and what version it would carry.

Read-only. Runs `git` and `cargo metadata`; writes nothing, tags nothing,
publishes nothing. Emits one JSON object on stdout so the same command that the
scheduled workflow runs can be run by hand against any range:

    .github/scripts/release-decide.py                      # last release tag → HEAD
    .github/scripts/release-decide.py --from A --to B      # any range
    .github/scripts/release-decide.py --to B               # first-release path

The two questions it answers are independent, and both must come back yes.

1. Is there a change worth releasing?

   A change is worth releasing when it can reach something a release ships:
   the crate tarballs on crates.io, or the binary attached to the GitHub
   release. Concretely, that is any file inside a workspace member's package
   directory, plus the root `Cargo.toml`, `Cargo.lock` and
   `rust-toolchain.toml`. The member directories come from `cargo metadata`,
   never from a list written down here — a crate that is added, moved, or
   merged into another one changes this set without anyone editing this file,
   which is the same property `publish-plan.py` has and for the same reason.

   Held-back (`publish = false`) members count too. They do not reach
   crates.io, but the CLI is what the binary release is built from, so a
   change to it does reach a consumer.

   Everything else does not: `.github/`, `docs/`, `specs/`, `harness/`, the
   root `README.md` and the root `LICENSE`. None of them are packaged and none
   of them change what a consumer downloads, so a release cut for one of them
   would be a version whose diff is empty from the outside. Note that a crate's
   *own* README is inside its package directory and therefore does count — it
   is the crates.io front page, and the maturity warning it carries is a claim
   the release makes.

2. Is there a commit that is not just the automation talking to itself?

   The version bump this automation pushes is itself a commit that touches
   `Cargo.toml` and `Cargo.lock` — both releasable paths. Left alone it would
   justify a release every week forever. Two things stop that: the tag is
   placed *on* the bump commit, so the next range starts after it and never
   contains it; and, belt and braces for a bump that was pushed but never
   tagged, any commit whose subject starts with `chore(release):` is excluded
   from the commit list outright. A range containing nothing else is not a
   release.

Version derivation follows conventional commits, with the pre-1.0 convention
applied deliberately rather than by accident. The workspace is `0.x`, where the
minor is what the major will be after 1.0, so every level shifts down one:

    major == 0        breaking → minor    feat → patch    other → patch
    major >= 1        breaking → major    feat → minor    other → patch

The `0.x` row is the rule already written down in docs/publishing.md
("treat a breaking change to any public API as a minor bump and everything
else as a patch bump"); this is that rule, executed. The `>= 1` row exists so
that crossing 1.0 does not silently keep shifting.

Breaking is `!` before the colon (`feat(core)!: …`) or a `BREAKING CHANGE:` /
`BREAKING-CHANGE:` line in the body. Merge commits are skipped: this repository
squash-merges, so they carry no type of their own and their content is already
in the range.
"""

import argparse
import json
import os
import re
import subprocess
import sys

# Repository-root files that are not inside any crate but do change what gets
# built or published. Everything else outside a member directory does not.
ROOT_RELEVANT = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")

# The subject this automation gives its own version-bump commit. Anything
# matching is the automation's own voice and never justifies the next release.
BUMP_SUBJECT = re.compile(r"^chore\(release\):")

RELEASE_TAG = re.compile(r"^v\d+\.\d+\.\d+$")

CONVENTIONAL = re.compile(
    r"^(?P<type>[A-Za-z]+)"
    r"(?:\((?P<scope>[^)]*)\))?"
    r"(?P<bang>!)?"
    r": (?P<desc>.+)$"
)

BREAKING_FOOTER = re.compile(r"^BREAKING[ -]CHANGE:", re.MULTILINE)

# Order is the order sections appear in the generated notes.
SECTIONS = [
    ("breaking", "Breaking changes"),
    ("feat", "Features"),
    ("fix", "Fixes"),
    ("perf", "Performance"),
    ("refactor", "Refactoring"),
    ("docs", "Documentation"),
    ("test", "Tests"),
    ("build", "Build"),
    ("ci", "CI"),
    ("chore", "Chores"),
    ("other", "Other"),
]


def git(*args):
    return subprocess.run(
        ["git", *args], check=True, capture_output=True, text=True
    ).stdout


def cargo_metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(out)


def relevant_paths(meta):
    """Git pathspecs for everything a release can ship, derived from the manifests."""
    root = meta["workspace_root"]
    dirs = {
        os.path.relpath(os.path.dirname(pkg["manifest_path"]), root)
        for pkg in meta["packages"]
    }
    return sorted(dirs) + list(ROOT_RELEVANT)


def last_release_tag(head):
    """The newest `v*.*.*` tag reachable from `head`, or None if there is none.

    Reachability matters: a tag on an abandoned branch is not this branch's
    last release, and taking it would silently shorten the range.
    """
    try:
        described = git("describe", "--tags", "--abbrev=0", "--match", "v*.*.*", head)
    except subprocess.CalledProcessError:
        return None
    tag = described.strip()
    return tag if RELEASE_TAG.match(tag) else None


def commits_in_range(base, head):
    """Non-merge, non-bump commits in `base..head`, oldest first."""
    span = f"{base}..{head}" if base else head
    sep = "\x1e"
    raw = git("log", "--no-merges", "--reverse", f"--format=%H%x1f%s%x1f%b{sep}", span)
    commits = []
    for record in raw.split(sep):
        record = record.strip("\n")
        if not record:
            continue
        sha, subject, body = record.split("\x1f", 2)
        if BUMP_SUBJECT.match(subject):
            continue
        commits.append(classify(sha, subject, body))
    return commits


def classify(sha, subject, body):
    match = CONVENTIONAL.match(subject)
    ctype = match.group("type").lower() if match else "other"
    breaking = bool(match and match.group("bang")) or bool(BREAKING_FOOTER.search(body))
    return {
        "sha": sha,
        "short": sha[:8],
        "subject": subject,
        "type": ctype,
        "scope": match.group("scope") if match else None,
        "description": match.group("desc") if match else subject,
        "breaking": breaking,
    }


def changed_paths(base, head, paths):
    if base is None:
        # No previous release: everything in the tree is new to a consumer.
        out = git("ls-tree", "-r", "--name-only", head, "--", *paths)
    else:
        out = git("diff", "--name-only", base, head, "--", *paths)
    return [line for line in out.splitlines() if line]


def bump_level(commits, current_major):
    if any(c["breaking"] for c in commits):
        return "minor" if current_major == 0 else "major"
    if any(c["type"] == "feat" for c in commits):
        return "patch" if current_major == 0 else "minor"
    return "patch"


def apply_bump(version, level):
    major, minor, patch = (int(part) for part in version.split("."))
    if level == "major":
        return f"{major + 1}.0.0"
    if level == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def workspace_version(meta):
    versions = sorted({pkg["version"] for pkg in meta["packages"]})
    if len(versions) != 1:
        detail = ", ".join(
            f"{p['name']} {p['version']}" for p in sorted(meta["packages"], key=lambda p: p["name"])
        )
        sys.exit(f"error: workspace members disagree on version: {detail}")
    return versions[0]


def render_notes(commits, version, base_tag, repo):
    buckets = {key: [] for key, _ in SECTIONS}
    for c in commits:
        key = "breaking" if c["breaking"] else (c["type"] if c["type"] in buckets else "other")
        buckets[key].append(c)

    lines = [f"## termproof {version}", ""]
    for key, heading in SECTIONS:
        entries = buckets[key]
        if not entries:
            continue
        lines.append(f"### {heading}")
        lines.append("")
        for c in entries:
            scope = f"**{c['scope']}:** " if c["scope"] else ""
            lines.append(f"- {scope}{c['description']} (`{c['short']}`)")
        lines.append("")

    if repo:
        if base_tag:
            lines.append(
                f"**Full changelog:** https://github.com/{repo}/compare/{base_tag}...v{version}"
            )
        else:
            lines.append(f"**Full changelog:** https://github.com/{repo}/commits/v{version}")
        lines.append("")

    lines.append(
        "Binaries for x86_64 Linux, x86_64 macOS and arm64 macOS are attached below "
        "with their SHA-256 checksums, and are built with provenance attestation."
    )
    return "\n".join(lines).rstrip() + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from", dest="base", help="baseline ref (default: last v*.*.* tag)")
    parser.add_argument("--to", dest="head", default="HEAD", help="head ref (default: HEAD)")
    parser.add_argument(
        "--no-auto-base",
        action="store_true",
        help="do not look for a baseline tag; treat this as the first release",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="release even when nothing releasable changed; the version rule still applies",
    )
    args = parser.parse_args()

    head = git("rev-parse", args.head).strip()
    base = args.base
    if base is None and not args.no_auto_base:
        base = last_release_tag(head)
    base_sha = git("rev-parse", base).strip() if base else None

    # A reversed range is silent otherwise: `git log` returns nothing (reading
    # as "quiet week") while `git diff` returns the whole change backwards.
    if base_sha and subprocess.run(
        ["git", "merge-base", "--is-ancestor", base_sha, head]
    ).returncode:
        sys.exit(f"error: {base} is not an ancestor of {args.head} — the range is reversed or unrelated")

    meta = cargo_metadata()
    current = workspace_version(meta)
    paths = relevant_paths(meta)

    commits = commits_in_range(base_sha, head)
    changed = changed_paths(base_sha, head, paths)

    first_release = base is None

    if not commits and not args.force:
        release, reason, level, nxt = (
            False,
            "no commits since the last release other than the automation's own version bump",
            "none",
            current,
        )
    elif not changed and not args.force:
        release, reason, level, nxt = (
            False,
            f"{len(commits)} commit(s) since {base}, none of them touching a path a "
            "release ships (crate directories, Cargo.toml, Cargo.lock, rust-toolchain.toml)",
            "none",
            current,
        )
    elif first_release:
        # Nothing has ever been released, so the version already in the manifest
        # is the one being reserved. Bumping past it would skip the number the
        # manifest and docs/publishing.md both name as the first release.
        release, reason, level, nxt = (
            True,
            "no previous release tag — releasing the version already in the manifest, unbumped",
            "none",
            current,
        )
    else:
        level = bump_level(commits, int(current.split(".")[0]))
        forced = " (forced)" if args.force and not changed else ""
        release, reason, nxt = (
            True,
            f"{len(commits)} commit(s) and {len(changed)} releasable path(s) "
            f"changed since {base}{forced}",
            apply_bump(current, level),
        )

    result = {
        "release": release,
        "reason": reason,
        "first_release": first_release,
        "base": base,
        "base_sha": base_sha,
        "head": head,
        "current_version": current,
        "next_version": nxt,
        "tag": f"v{nxt}",
        "bump": level,
        "commit_count": len(commits),
        "commits": commits,
        "relevant_paths": paths,
        "changed_paths": changed,
        "notes": render_notes(commits, nxt, base, os.environ.get("GITHUB_REPOSITORY", ""))
        if release
        else "",
    }
    json.dump(result, sys.stdout, indent=2)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
