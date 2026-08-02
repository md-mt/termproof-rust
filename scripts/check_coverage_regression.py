#!/usr/bin/env python3
"""Coverage non-regression gate for the TermProof Rust workspace (RUST-003).

Replaces the old fixed ``--fail-under-lines 90`` floor with a true
non-regression gate: the committed, machine-readable baseline at
``rust/coverage/baseline.json`` records the exact line-coverage totals
measured at the M0 baseline (100% of 6 lines across the workspace), and
this script fails whenever the current measurement drops below ANY of the
recorded totals (workspace-wide or per crate). A drop can never pass, even
if it stays above an arbitrary percentage.

Usage:
    # Generate/refresh the committed baseline from a current measurement:
    cargo llvm-cov --workspace --json --output-path current.json
    python3 scripts/check_coverage_regression.py --generate \\
        --current current.json --baseline coverage/baseline.json

    # Gate (CI):
    cargo llvm-cov --workspace --json --output-path current.json
    python3 scripts/check_coverage_regression.py \\
        --current current.json --baseline coverage/baseline.json

Exit status:
    0  coverage is at or above baseline everywhere
    1  usage/environment error
    2  coverage dropped below baseline (workspace and/or crate level)
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, NoReturn


def die(message: str, code: int = 1) -> NoReturn:
    print(f"coverage-regression: {message}", file=sys.stderr)
    raise SystemExit(code)


def load_json(path: Path, label: str) -> dict[str, Any]:
    if not path.is_file():
        die(f"{label} file {path} is missing", 1)
    try:
        raw = path.read_text(encoding="utf-8")
    except OSError as exc:
        die(f"{label} file {path} cannot be read: {exc}", 1)
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        die(f"{label} file {path} is not valid JSON: {exc}", 1)
    if not isinstance(data, dict):
        die(f"{label} file {path} is not a JSON object", 1)
    return data


def totals_from_llvm_cov(llvm_cov: dict[str, Any]) -> tuple[dict[str, Any], dict[str, dict[str, Any]]]:
    """Extract workspace totals + per-crate line totals from cargo llvm-cov JSON."""
    data = llvm_cov.get("data")
    if not isinstance(data, list) or not data:
        die("cargo llvm-cov JSON has no data array", 1)
    totals = data[0].get("totals", {})
    lines = totals.get("lines", {})
    if "count" not in lines or "covered" not in lines:
        die("cargo llvm-cov JSON totals.lines missing count/covered", 1)

    crates: dict[str, dict[str, Any]] = {}
    for entry in data[0].get("files", []):
        filename = entry.get("filename", "")
        marker = "/crates/"
        idx = filename.rfind(marker)
        if idx == -1:
            continue
        rest = filename[idx + len(marker):]
        crate = rest.split("/")[0]
        file_lines = (entry.get("summary") or {}).get("lines") or {}
        count = file_lines.get("count", 0)
        covered = file_lines.get("covered", 0)
        slot = crates.setdefault(crate, {"count": 0, "covered": 0})
        slot["count"] += count
        slot["covered"] += covered

    for slot in crates.values():
        slot["percent"] = round(100.0 * slot["covered"] / slot["count"], 2) if slot["count"] else 0.0
    return lines, crates


def check_level(name: str, baseline: dict[str, Any], current: dict[str, Any]) -> list[str]:
    """Return failure strings if current line totals drop below baseline.

    This is a true non-regression gate: adding fully-covered lines (count
    grows while percent stays 100) is allowed, but any drop in covered lines
    or in line percent fails.
    """
    problems: list[str] = []
    b_count = baseline.get("count", 0)
    b_covered = baseline.get("covered", 0)
    b_percent = baseline.get("percent", 0.0)
    c_count = current.get("count", 0)
    c_covered = current.get("covered", 0)
    c_percent = current.get("percent", 0.0)
    if c_covered < b_covered:
        problems.append(
            f"{name}: covered lines dropped {b_covered} -> {c_covered} (below baseline)"
        )
    if c_percent < b_percent - 1e-9:
        problems.append(
            f"{name}: line percent dropped {b_percent}% -> {c_percent}% (below baseline)"
        )
    if c_count < b_count:
        problems.append(
            f"{name}: line count shrank {b_count} -> {c_count} (coverage surface removed)"
        )
    return problems


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        description="Fail when Rust coverage drops below the committed M0 baseline."
    )
    parser.add_argument("--current", required=True, help="cargo llvm-cov --json output")
    parser.add_argument("--baseline", required=True, help="committed baseline JSON")
    parser.add_argument(
        "--generate",
        action="store_true",
        help="regenerate the baseline from --current (deliberate, reviewed act)",
    )
    args = parser.parse_args(argv)

    current_path = Path(args.current)
    baseline_path = Path(args.baseline)
    current = load_json(current_path, "current coverage")

    if args.generate:
        lines, crates = totals_from_llvm_cov(current)
        baseline = {
            "schema_version": 1,
            "tool": {
                "name": "cargo-llvm-cov",
                "version": (current.get("cargo_llvm_cov") or {}).get("version", "unknown"),
            },
            "generated_at": "",
            "note": (
                "Committed M0 coverage baseline. check_coverage_regression.py fails on any "
                "drop below these exact covered/count values or line percent. Regenerate "
                "deliberately with --generate when coverage intentionally increases."
            ),
            "workspace": {"lines": lines},
            "crates": {name: {"lines": c} for name, c in crates.items()},
        }
        baseline_path.write_text(json.dumps(baseline, indent=2) + "\n", encoding="utf-8")
        print(
            f"coverage-regression: baseline regenerated at {baseline_path} "
            f"(workspace {lines['covered']}/{lines['count']} lines, {lines.get('percent')}%)"
        )
        return

    baseline = load_json(baseline_path, "baseline")
    lines, crates = totals_from_llvm_cov(current)
    workspace_baseline = baseline.get("workspace", {}).get("lines", {})
    crates_baseline = baseline.get("crates", {})

    problems: list[str] = []
    problems.extend(check_level("workspace", workspace_baseline, lines))
    for crate, current_crate in crates.items():
        base_crate = crates_baseline.get(crate, {}).get("lines", {})
        problems.extend(check_level(f"crate {crate}", base_crate, current_crate))
    # A crate in the baseline that disappears entirely is also a regression.
    for crate in crates_baseline:
        if crate not in crates:
            problems.append(f"crate {crate}: no longer measured (missing from current coverage)")

    for problem in problems:
        print(f"coverage-regression: FAIL {problem}", file=sys.stderr)
    if problems:
        die(f"{len(problems)} coverage regression(s) detected", 2)

    print(
        f"coverage-regression: OK — workspace {lines['covered']}/{lines['count']} lines "
        f"({lines.get('percent')}%) at or above committed baseline"
    )


if __name__ == "__main__":
    main()
