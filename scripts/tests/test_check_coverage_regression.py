"""Adversarial tests for the Rust coverage non-regression gate.

The gate must fail when coverage drops below the committed machine-readable
baseline — including a drop that stays above any percentage floor — and must
also fail when the line count changes or a measured crate disappears.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "rust" / "scripts" / "check_coverage_regression.py"

BASELINE = {
    "schema_version": 1,
    "tool": {"name": "cargo-llvm-cov", "version": "0.8.7"},
    "workspace": {"lines": {"count": 6, "covered": 6, "percent": 100}},
    "crates": {
        "termproof-cli": {"lines": {"count": 3, "covered": 3, "percent": 100.0}},
        "termproof-core": {"lines": {"count": 3, "covered": 3, "percent": 100.0}},
    },
}


def llvm_cov_json(files: list[tuple[str, int, int]]) -> dict:
    """Build a minimal cargo llvm-cov --json document.

    files: list of (filename, count, covered).
    """
    total_count = sum(c for _, c, _ in files)
    total_covered = sum(c for _, _, c in files)
    return {
        "type": "llvm.coverage.json.export",
        "version": "2.0.0",
        "cargo_llvm_cov": {"version": "0.8.7"},
        "data": [
            {
                "totals": {
                    "lines": {
                        "count": total_count,
                        "covered": total_covered,
                        "percent": round(100.0 * total_covered / total_count, 2)
                        if total_count
                        else 0.0,
                    }
                },
                "files": [
                    {
                        "filename": f"/repo/rust/crates/{crate}/src/main.rs",
                        "summary": {"lines": {"count": c, "covered": k, "percent": 100.0 if c and c == k else 0.0}},
                    }
                    for crate, c, k in files
                ],
            }
        ],
    }


def run_gate(baseline_path: Path, current_path: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [
            sys.executable,
            str(SCRIPT),
            "--baseline",
            str(baseline_path),
            "--current",
            str(current_path),
        ],
        capture_output=True,
        text=True,
    )


class CoverageRegressionGateTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.baseline = self.root / "baseline.json"
        self.current = self.root / "current.json"
        self.baseline.write_text(json.dumps(BASELINE), encoding="utf-8")

    def write_current(self, files: list[tuple[str, int, int]]) -> None:
        self.current.write_text(json.dumps(llvm_cov_json(files)), encoding="utf-8")

    def test_baseline_equal_passes(self) -> None:
        self.write_current(
            [
                ("termproof-cli", 3, 3),
                ("termproof-core", 3, 3),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_drop_below_baseline_fails_even_above_any_floor(self) -> None:
        # 5/6 = 83.3% — well above the old 90% floor would have passed it,
        # but any drop below the 100% committed baseline must fail.
        self.write_current(
            [
                ("termproof-cli", 3, 3),
                ("termproof-core", 3, 2),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("covered lines dropped", result.stderr)
        self.assertIn("termproof-core", result.stderr)

    def test_crate_level_drop_fails(self) -> None:
        self.write_current(
            [
                ("termproof-cli", 3, 2),
                ("termproof-core", 3, 3),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("termproof-cli", result.stderr)

    def test_workspace_drop_fails(self) -> None:
        self.write_current(
            [
                ("termproof-cli", 3, 2),
                ("termproof-core", 3, 2),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("workspace", result.stderr)

    def test_growth_with_full_coverage_passes(self) -> None:
        # Adding fully-covered lines grows the count but does NOT drop below
        # the baseline: a true non-regression gate must allow this.
        self.write_current(
            [
                ("termproof-cli", 5, 5),
                ("termproof-core", 3, 3),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_coverage_surface_shrink_fails(self) -> None:
        # Removing covered lines (count shrank) must fail even if percent is 100.
        self.write_current(
            [
                ("termproof-cli", 3, 3),
                ("termproof-core", 2, 2),
            ]
        )
        result = run_gate(self.baseline, self.current)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("coverage surface removed", result.stderr)
        self.assertIn("termproof-core", result.stderr)

    def test_missing_crate_fails(self) -> None:
        self.write_current([("termproof-cli", 3, 3)])
        result = run_gate(self.baseline, self.current)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no longer measured", result.stderr)

    def test_generate_writes_baseline(self) -> None:
        self.write_current(
            [
                ("termproof-cli", 3, 3),
                ("termproof-core", 3, 3),
            ]
        )
        result = subprocess.run(
            [
                sys.executable,
                str(SCRIPT),
                "--generate",
                "--baseline",
                str(self.root / "generated.json"),
                "--current",
                str(self.current),
            ],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        generated = json.loads((self.root / "generated.json").read_text())
        self.assertEqual(generated["workspace"]["lines"]["covered"], 6)
        self.assertEqual(generated["workspace"]["lines"]["percent"], 100)

    def test_missing_current_fails(self) -> None:
        result = run_gate(self.baseline, self.root / "nope.json")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing", result.stderr)


if __name__ == "__main__":
    unittest.main()
