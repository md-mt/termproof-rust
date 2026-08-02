#!/usr/bin/env python3
"""Conformance gate for the TermProof Rust workspace (RUST-003, issue #96).

Executes the real compiled `termproof` binary against a checked-in golden
corpus. Corpus layout (one case per basename):

    <name>.stdout.golden   required; exact expected stdout bytes
    <name>.args            optional; one argument per line
    <name>.exit            optional; expected exit code (default 0)

Exit status:
  0  every case passed
  1  usage or environment error (missing binary/corpus, empty corpus)
  2  one or more cases failed (stdout and/or exit-code mismatch)
"""

import subprocess
import sys
from argparse import ArgumentParser
from pathlib import Path
from typing import NoReturn


def die(message: str, code: int = 1) -> NoReturn:
    print(f"conformance: {message}", file=sys.stderr)
    raise SystemExit(code)


def discover_cases(corpus: Path) -> list[str]:
    """Return case basenames derived from *.stdout.golden files."""
    if not corpus.is_dir():
        die(f"conformance corpus {corpus} is missing")
    goldens = sorted(corpus.glob("*.stdout.golden"))
    if not goldens:
        die(f"no conformance cases found in {corpus}")
    return [golden.name[: -len(".stdout.golden")] for golden in goldens]


def orphaned_arg_files(corpus: Path, cases: list[str]) -> list[str]:
    """Arg files without a matching golden indicate a broken case."""
    orphans: list[str] = []
    for args_file in corpus.glob("*.args"):
        name = args_file.name[: -len(".args")]
        if name not in cases:
            orphans.append(args_file.name)
    return orphans


def run_case(binary: Path, corpus: Path, name: str) -> tuple[int, bytes]:
    """Run the binary for one case and return (exit_code, stdout_bytes)."""
    argv = [str(binary)]
    args_file = corpus / f"{name}.args"
    if args_file.is_file():
        argv.extend(
            line.strip()
            for line in args_file.read_text(encoding="utf-8").splitlines()
            if line.strip()
        )
    try:
        completed = subprocess.run(argv, capture_output=True, timeout=60)
    except OSError as exc:
        die(f"cannot execute binary {binary}: {exc}")
    except subprocess.TimeoutExpired:
        die(f"binary {binary} timed out on case {name}")
    return completed.returncode, completed.stdout


def expected_exit(corpus: Path, name: str) -> int:
    exit_file = corpus / f"{name}.exit"
    if not exit_file.is_file():
        return 0
    raw = exit_file.read_text(encoding="utf-8").strip()
    try:
        return int(raw)
    except ValueError:
        die(f"exit file {exit_file} is not an integer: {raw!r}")


def check_case(binary: Path, corpus: Path, name: str) -> list[str]:
    golden = (corpus / f"{name}.stdout.golden").read_bytes()
    actual_code, actual_stdout = run_case(binary, corpus, name)
    expected = expected_exit(corpus, name)
    problems: list[str] = []
    if actual_code != expected:
        problems.append(
            f"{name}: exit code {actual_code} != expected {expected}"
        )
    if actual_stdout != golden:
        problems.append(f"{name}: stdout differs from {name}.stdout.golden")
    return problems


def main(argv: list[str] | None = None) -> None:
    parser = ArgumentParser(
        description="Run the compiled termproof binary against a golden "
        "conformance corpus."
    )
    parser.add_argument("--binary", required=True, help="path to termproof binary")
    parser.add_argument("--corpus", required=True, help="path to conformance corpus")
    args = parser.parse_args(argv)

    binary = Path(args.binary)
    corpus = Path(args.corpus)
    cases = discover_cases(corpus)
    orphans = orphaned_arg_files(corpus, cases)
    for orphan in orphans:
        print(f"conformance: orphaned args file without golden: {orphan}", file=sys.stderr)
    if orphans:
        die("orphaned args file(s) present", 2)

    failures: list[str] = []
    for name in cases:
        failures.extend(check_case(binary, corpus, name))
    for failure in failures:
        print(f"conformance: FAIL {failure}", file=sys.stderr)
    if failures:
        die(f"{len(failures)} conformance case(s) failed", 2)
    print(
        f"conformance: OK — {len(cases)} case(s) passed against {binary}: "
        f"{', '.join(cases)}"
    )


if __name__ == "__main__":
    main()
