"""Adversarial tests for the cross-runtime conformance gate (RUST-003).

The gate runs the SAME machine-readable M0 corpus through an explicit
Python-oracle adapter and an explicit Rust-side adapter, produces a
machine-readable difference report, and fails on semantic drift. These tests
prove that mismatches fail, that the report records the differences, and that
the oracle is genuinely exercised (not just a golden self-check).
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "rust" / "scripts" / "check_conformance.py"

PASSING_CORPUS = {
    "schema_version": 1,
    "cases": [
        {
            "id": "help-root",
            "argv": ["--help"],
            "semantic": {
                "exit_code": 0,
                "stdout_nonempty": True,
                "stderr_empty": True,
                "stdout_contains": ["termproof"],
            },
        }
    ],
}


def write_script(path: Path, body: str) -> None:
    path.write_text("#!/usr/bin/env python3\n" + body, encoding="utf-8")
    path.chmod(0o755)


def write_fake_runtimes(root: Path, rust_exit: int = 0, oracle_exit: int = 0) -> tuple[Path, Path]:
    """Create a fake rust binary and a fake python oracle CLI.

    The fake rust binary prints a greeting and exits rust_exit.
    The fake oracle prints 'usage: termproof' (contains 'termproof') and
    exits oracle_exit.
    """
    rust_bin = root / "fake-termproof"
    write_script(
        rust_bin,
        "import sys\n"
        f"print('termproof 0.1.0 (rust workspace baseline)')\n"
        f"sys.exit({rust_exit})\n",
    )
    oracle_bin = root / "fake-oracle"
    write_script(
        oracle_bin,
        "import sys\n"
        "print('usage: termproof [-h] {run,list,validate,plugins,init,demo} ...')\n"
        f"sys.exit({oracle_exit})\n",
    )
    return rust_bin, oracle_bin


def run_gate(
    root: Path,
    corpus: dict,
    rust_bin: Path,
    oracle_bin: Path,
    report: Path | None = None,
) -> subprocess.CompletedProcess:
    corpus_path = root / "corpus.json"
    corpus_path.write_text(json.dumps(corpus), encoding="utf-8")
    cmd = [
        sys.executable,
        str(SCRIPT),
        "--corpus",
        str(corpus_path),
        "--binary",
        str(rust_bin),
        "--oracle",
        f"{sys.executable} {oracle_bin}",
    ]
    if report is not None:
        cmd.extend(["--report", str(report)])
    return subprocess.run(cmd, capture_output=True, text=True)


class CrossRuntimeConformanceTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)

    def test_passing_cases_pass_and_report_is_machine_readable(self) -> None:
        rust_bin, oracle_bin = write_fake_runtimes(self.root)
        report = self.root / "report.json"
        result = run_gate(self.root, PASSING_CORPUS, rust_bin, oracle_bin, report)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PASS", result.stdout)
        data = json.loads(report.read_text())
        self.assertEqual(data["summary"]["passed"], 1)
        self.assertEqual(data["summary"]["failed"], 0)
        self.assertEqual(data["cases"][0]["verdict"], "PASS")

    def test_rust_mismatch_fails_and_report_records_difference(self) -> None:
        # Rust exits 7: semantic drift must fail and the report must record it.
        rust_bin, oracle_bin = write_fake_runtimes(self.root, rust_exit=7)
        report = self.root / "report.json"
        result = run_gate(self.root, PASSING_CORPUS, rust_bin, oracle_bin, report)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exit code 7", result.stderr)
        data = json.loads(report.read_text())
        self.assertEqual(data["summary"]["failed"], 1)
        self.assertIn("rust", data["cases"][0]["differences"][0])

    def test_oracle_mismatch_fails(self) -> None:
        # The oracle itself regressed (exit 3): the gate must catch it too.
        rust_bin, oracle_bin = write_fake_runtimes(self.root, oracle_exit=3)
        result = run_gate(self.root, PASSING_CORPUS, rust_bin, oracle_bin)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exit code 3", result.stderr)
        self.assertIn("python", result.stderr)

    def test_empty_stdout_fails(self) -> None:
        rust_bin = self.root / "fake-termproof"
        write_script(rust_bin, "import sys\nsys.exit(0)\n")  # no stdout
        oracle_bin = self.root / "fake-oracle"
        write_script(
            oracle_bin,
            "import sys\nprint('usage: termproof ...')\nsys.exit(0)\n",
        )
        result = run_gate(self.root, PASSING_CORPUS, rust_bin, oracle_bin)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stdout empty", result.stderr)

    def test_stderr_not_empty_fails(self) -> None:
        rust_bin = self.root / "fake-termproof"
        write_script(
            rust_bin,
            "import sys\nprint('termproof 0.1.0 (rust workspace baseline)')\n"
            "print('error on stderr', file=sys.stderr)\nsys.exit(0)\n",
        )
        oracle_bin = self.root / "fake-oracle"
        write_script(
            oracle_bin,
            "import sys\nprint('usage: termproof ...')\nsys.exit(0)\n",
        )
        result = run_gate(self.root, PASSING_CORPUS, rust_bin, oracle_bin)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("stderr non-empty", result.stderr)

    def test_missing_binary_fails(self) -> None:
        _, oracle_bin = write_fake_runtimes(self.root)
        result = run_gate(self.root, PASSING_CORPUS, self.root / "missing", oracle_bin)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("binary", result.stderr)

    def test_empty_corpus_fails(self) -> None:
        rust_bin, oracle_bin = write_fake_runtimes(self.root)
        result = run_gate(self.root, {"schema_version": 1, "cases": []}, rust_bin, oracle_bin)
        # Zero cases is a configuration error, not a silent pass.
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no cases", result.stderr)


if __name__ == "__main__":
    unittest.main()
