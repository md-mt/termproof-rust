"""Unit tests for the Rust workspace conformance gate.

The conformance gate (RUST-003, issue #96) executes the real compiled
`termproof` binary against a checked-in golden corpus. Each case consists of an
optional args file, an exact stdout golden, and an optional expected exit code.
A case passes only when stdout and the exit code match the golden bytes
exactly; the harness reports every mismatch and exits non-zero when any case
fails.
"""

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "rust" / "scripts" / "check_conformance.py"

FAKE_BIN = """#!/usr/bin/env python3
import sys

# Deterministic fake binary for the harness tests. Mirrors the shape of the
# RUST-002 baseline greeting contract: prints a fixed banner and exits 0.
print("fake-termproof 0.1.0 (test fixture)")
sys.exit(0)
"""


def write_fake_binary(directory: Path) -> Path:
    binary = directory / "fake-termproof"
    binary.write_text(FAKE_BIN, encoding="utf-8")
    binary.chmod(0o755)
    return binary


def run_script(*args: str, cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )


class ConformanceHarnessTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.binary = write_fake_binary(self.root)
        self.corpus = self.root / "corpus"
        self.corpus.mkdir()

    def add_case(self, name: str, stdout: str, args: str | None = None,
                 exit_code: str | None = None) -> None:
        (self.corpus / f"{name}.stdout.golden").write_text(stdout, encoding="utf-8")
        if args is not None:
            (self.corpus / f"{name}.args").write_text(args, encoding="utf-8")
        if exit_code is not None:
            (self.corpus / f"{name}.exit").write_text(exit_code, encoding="utf-8")

    def run_gate(self) -> subprocess.CompletedProcess:
        return run_script(
            "--binary", str(self.binary),
            "--corpus", str(self.corpus),
            cwd=self.root,
        )

    def test_matching_case_passes(self) -> None:
        self.add_case("baseline", "fake-termproof 0.1.0 (test fixture)\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("baseline", result.stdout)

    def test_stdout_mismatch_fails(self) -> None:
        self.add_case("baseline", "different output\n")
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("baseline", result.stderr)
        self.assertIn("stdout", result.stderr)

    def test_exit_code_mismatch_fails(self) -> None:
        self.add_case("baseline", "fake-termproof 0.1.0 (test fixture)\n",
                      exit_code="7")
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exit", result.stderr)

    def test_matching_exit_code_passes(self) -> None:
        self.add_case("baseline", "fake-termproof 0.1.0 (test fixture)\n",
                      exit_code="0")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_args_are_passed_through(self) -> None:
        # The fake binary ignores args; this proves the args file is accepted
        # without the harness treating an args file as a case of its own.
        self.add_case("with_args", "fake-termproof 0.1.0 (test fixture)\n",
                      args="--help\n")
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_empty_corpus_is_an_error(self) -> None:
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no conformance cases", result.stderr)

    def test_missing_binary_fails(self) -> None:
        self.add_case("baseline", "fake-termproof 0.1.0 (test fixture)\n")
        result = run_script(
            "--binary", str(self.root / "does-not-exist"),
            "--corpus", str(self.corpus),
            cwd=self.root,
        )
        self.assertNotEqual(result.returncode, 0)

    def test_missing_golden_fails(self) -> None:
        # An args file without a matching golden is a broken case, even when
        # other valid cases exist.
        self.add_case("baseline", "fake-termproof 0.1.0 (test fixture)\n")
        (self.corpus / "orphan.args").write_text("--version\n", encoding="utf-8")
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("orphan", result.stderr)

    def test_corpus_is_required(self) -> None:
        result = run_script("--binary", str(self.binary), cwd=self.root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--corpus", result.stderr)


if __name__ == "__main__":
    unittest.main()
