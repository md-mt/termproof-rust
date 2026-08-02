#!/usr/bin/env python3
"""Cross-runtime conformance gate for the TermProof Rust workspace (RUST-003).

Implements spec section 6.1: the SAME machine-readable M0 corpus cases are
executed through an explicit Python-oracle adapter and an explicit Rust-side
adapter, and the gate produces a machine-readable difference report and fails
on semantic drift.

Scope guard (RUST-002 skeleton): the Rust binary only implements the M0
greeting contract and does not yet parse subcommands (CLI parity lands in
RUST-015/M2). The corpus therefore contains the invocation surface where
Python and Rust are REQUIRED to agree at M0: help-style invocations must
succeed (exit 0), write non-empty program-identifying output to stdout, and
keep stderr empty. As later milestones implement behavior, corpus cases are
added/tightened here and the difference report grows.

Corpus schema (rust/conformance/corpus.json):
    {
      "schema_version": 1,
      "cases": [
        {
          "id": "help-root",
          "argv": ["--help"],
          "semantic": {
            "exit_code": 0,
            "stdout_nonempty": true,
            "stderr_empty": true,
            "stdout_contains": ["termproof"]
          },
          "note": "human-readable intent"
        }
      ]
    }

The difference report (--report) is machine-readable JSON:
    {
      "schema_version": 1,
      "python_oracle": "<command>",
      "rust_binary": "<path>",
      "cases": [
        {
          "id": ...,
          "argv": [...],
          "python": {"exit_code": ..., "stdout": "...", "stderr": "..."},
          "rust": {"exit_code": ..., "stdout": "...", "stderr": "..."},
          "verdict": "PASS" | "FAIL",
          "differences": ["human-readable drift description"]
        }
      ],
      "summary": {"total": N, "passed": N, "failed": N}
    }

Exit status:
    0  every case passed on both runtimes
    1  usage or environment error (missing corpus/binary/oracle)
    2  one or more cases failed (semantic drift detected)
"""

from __future__ import annotations

import json
import shlex
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, NoReturn


def die(message: str, code: int = 1) -> NoReturn:
    print(f"conformance: {message}", file=sys.stderr)
    raise SystemExit(code)


@dataclass
class Observation:
    exit_code: int
    stdout: str
    stderr: str


@dataclass
class Adapter:
    """Base interface for an explicit runtime adapter."""

    name: str

    def observe(self, argv: list[str]) -> Observation:
        raise NotImplementedError


@dataclass
class PythonOracleAdapter(Adapter):
    """Explicit Python-oracle adapter: runs the real Python ``termproof`` CLI."""

    oracle_command: list[str]

    def observe(self, argv: list[str]) -> Observation:
        completed = subprocess.run(
            [*self.oracle_command, *argv],
            capture_output=True,
            text=True,
            timeout=120,
        )
        return Observation(completed.returncode, completed.stdout, completed.stderr)


@dataclass
class RustBinaryAdapter(Adapter):
    """Explicit Rust-side adapter: runs the compiled ``termproof`` binary."""

    binary: Path

    def observe(self, argv: list[str]) -> Observation:
        try:
            completed = subprocess.run(
                [str(self.binary), *argv],
                capture_output=True,
                text=True,
                timeout=120,
            )
        except OSError as exc:
            die(f"cannot execute binary {self.binary}: {exc}")
        return Observation(completed.returncode, completed.stdout, completed.stderr)


def load_corpus(path: Path) -> dict[str, Any]:
    if not path.is_file():
        die(f"conformance corpus {path} is missing")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        die(f"conformance corpus {path} is not valid JSON: {exc}")
    if not isinstance(data, dict) or not isinstance(data.get("cases"), list):
        die(f"conformance corpus {path} must be an object with a cases array")
    return data


def evaluate_semantic(
    semantic: dict[str, Any], python: Observation, rust: Observation
) -> list[str]:
    """Return drift descriptions if either runtime violates the semantic contract.

    Every key in ``semantic`` must hold for BOTH runtimes. Returns [] when the
    case passes; otherwise a list of human/machine-readable failures.
    """
    problems: list[str] = []
    for runtime, obs in (("python", python), ("rust", rust)):
        label = f"{runtime} (exit {obs.exit_code})"
        if "exit_code" in semantic and obs.exit_code != semantic["exit_code"]:
            problems.append(
                f"{runtime}: exit code {obs.exit_code} != required {semantic['exit_code']}"
            )
        if semantic.get("stdout_nonempty") and not obs.stdout:
            problems.append(f"{runtime}: stdout empty but required non-empty")
        if semantic.get("stdout_empty") and obs.stdout:
            problems.append(f"{runtime}: stdout non-empty but required empty")
        if semantic.get("stderr_empty") and obs.stderr:
            problems.append(f"{runtime}: stderr non-empty but required empty")
        for needle in semantic.get("stdout_contains", []):
            if needle not in obs.stdout:
                problems.append(f"{runtime}: stdout missing {needle!r}")
        for needle in semantic.get("stderr_contains", []):
            if needle not in obs.stderr:
                problems.append(f"{runtime}: stderr missing {needle!r}")
    return problems


def run_case(case: dict[str, Any], adapters: dict[str, Adapter]) -> dict[str, Any]:
    argv = [str(a) for a in case.get("argv", [])]
    python = adapters["python"].observe(argv)
    rust = adapters["rust"].observe(argv)
    semantic = case.get("semantic", {})
    problems = evaluate_semantic(semantic, python, rust)
    verdict = "FAIL" if problems else "PASS"
    return {
        "id": case.get("id", "<unnamed>"),
        "argv": argv,
        "python": {"exit_code": python.exit_code, "stdout": python.stdout, "stderr": python.stderr},
        "rust": {"exit_code": rust.exit_code, "stdout": rust.stdout, "stderr": rust.stderr},
        "semantic": semantic,
        "verdict": verdict,
        "differences": problems,
    }


def main(argv: list[str] | None = None) -> None:
    parser = argparse_parser()
    args = parser.parse_args(argv)

    corpus_path = Path(args.corpus)
    corpus = load_corpus(corpus_path)
    cases = corpus.get("cases", [])
    if not cases:
        die("conformance corpus has no cases; add corpus cases before running the gate")

    binary = Path(args.binary)
    if not binary.is_file():
        die(f"rust binary {binary} is missing; build it first (cargo build -p termproof-cli)")

    oracle_command = shlex.split(args.oracle)
    if not oracle_command:
        die("--oracle command is empty")

    adapters: dict[str, Adapter] = {
        "python": PythonOracleAdapter(name="python-oracle", oracle_command=oracle_command),
        "rust": RustBinaryAdapter(name="rust-side", binary=binary),
    }

    results = [run_case(case, adapters) for case in cases]
    passed = sum(1 for r in results if r["verdict"] == "PASS")
    failed = len(results) - passed

    report = {
        "schema_version": corpus.get("schema_version", 1),
        "python_oracle": args.oracle,
        "rust_binary": str(binary),
        "cases": results,
        "summary": {"total": len(results), "passed": passed, "failed": failed},
    }
    if args.report:
        report_path = Path(args.report)
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        print(f"conformance: difference report written to {report_path}")

    for result in results:
        if result["verdict"] == "FAIL":
            print(
                f"conformance: FAIL {result['id']}: "
                + "; ".join(result["differences"]),
                file=sys.stderr,
            )
        else:
            print(
                f"conformance: PASS {result['id']} "
                f"(python exit {result['python']['exit_code']}, "
                f"rust exit {result['rust']['exit_code']})"
            )
    print(
        f"conformance: {passed}/{len(results)} case(s) passed on both runtimes"
    )
    if failed:
        raise SystemExit(2)


def argparse_parser():
    from argparse import ArgumentParser

    parser = ArgumentParser(
        description="Run the same M0 conformance corpus through the Python oracle "
        "and the Rust binary; fail on semantic drift."
    )
    parser.add_argument("--corpus", required=True, help="path to conformance corpus JSON")
    parser.add_argument("--binary", required=True, help="path to compiled termproof binary")
    parser.add_argument(
        "--oracle",
        default="uv run python -m termproof",
        help="shell command that runs the Python oracle CLI (default: uv run python -m termproof)",
    )
    parser.add_argument(
        "--report",
        help="optional path to write the machine-readable difference report JSON",
    )
    return parser


if __name__ == "__main__":
    main()
