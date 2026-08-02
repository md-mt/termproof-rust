#!/usr/bin/env python3
"""Schema-drift gate for the TermProof Rust workspace (RUST-003, issue #96).

The canonical recipe schema is the Draft 2020-12 document checked in at
docs/recipe-schema-v1.json. Any JSON schema copy carried inside the Rust
workspace must remain byte-identical to it, so a schema change can never
silently diverge between the Python oracle and the Rust implementation.

Exit status:
  0  every check passed
  1  the canonical schema is missing, not valid JSON, or not Draft 2020-12
  2  one or more Rust workspace schema copies drifted from the canonical file
"""

import json
import sys
from argparse import ArgumentParser
from pathlib import Path
from typing import NoReturn

DRAFT_2020_12 = "https://json-schema.org/draft/2020-12/schema"
# Schema copies inside the workspace: any JSON file whose name contains
# "schema" (for example crates/termproof-core/resources/recipe-schema-v1.json).
SCHEMA_GLOB = "*schema*.json"


def die(message: str, code: int = 1) -> NoReturn:
    print(f"schema-drift: {message}", file=sys.stderr)
    raise SystemExit(code)


def canonical_ok(canonical: Path) -> dict:
    """Validate the canonical schema file and return its parsed document."""
    if not canonical.is_file():
        die(f"canonical schema {canonical} is missing")
    try:
        raw = canonical.read_text(encoding="utf-8")
    except OSError as exc:
        die(f"canonical schema {canonical} cannot be read: {exc}")
    try:
        document = json.loads(raw)
    except json.JSONDecodeError as exc:
        die(f"canonical schema {canonical} is not valid JSON: {exc}")
    if not isinstance(document, dict):
        die(f"canonical schema {canonical} is not a JSON object")
    if document.get("$schema") != DRAFT_2020_12:
        die(
            f"canonical schema {canonical} must declare $schema "
            f"{DRAFT_2020_12!r}, found {document.get('$schema')!r}"
        )
    return document


def find_schema_copies(rust_root: Path) -> list[Path]:
    """Return workspace schema files, excluding build output under target/."""
    if not rust_root.is_dir():
        die(f"rust workspace root {rust_root} is missing")
    copies: list[Path] = []
    for path in rust_root.rglob(SCHEMA_GLOB):
        if not path.is_file():
            continue
        if "target" in path.parts:
            continue
        copies.append(path)
    return copies


def check_copies(canonical: Path, copies: list[Path]) -> list[str]:
    canonical_bytes = canonical.read_bytes()
    drift: list[str] = []
    for copy in copies:
        if copy.read_bytes() != canonical_bytes:
            drift.append(
                f"schema drift: {copy} differs from canonical {canonical}"
            )
    return drift


def main(argv: list[str] | None = None) -> None:
    parser = ArgumentParser(
        description="Verify the Rust workspace schema has not drifted from the "
        "canonical Draft 2020-12 recipe schema."
    )
    parser.add_argument(
        "--canonical",
        required=True,
        help="path to the canonical Draft 2020-12 recipe schema",
    )
    parser.add_argument(
        "--rust-root",
        required=True,
        help="path to the Rust workspace to scan for schema copies",
    )
    args = parser.parse_args(argv)

    canonical = Path(args.canonical)
    rust_root = Path(args.rust_root)
    canonical_ok(canonical)

    copies = find_schema_copies(rust_root)
    drift = check_copies(canonical, copies)
    for message in drift:
        print(message, file=sys.stderr)
    if drift:
        raise SystemExit(2)
    print(
        f"schema-drift: OK — canonical {canonical} is valid Draft 2020-12, "
        f"{len(copies)} workspace copy/copies in sync"
    )


if __name__ == "__main__":
    main()
