"""Audit the Rust CI workflow path filters (RUST-003, finding 2).

Every canonical input consumed by a Rust CI gate must be present in the
pull_request and push path filters, so a change to that input triggers the
gate that consumes it. The schema-drift gate reads
docs/recipe-schema-v1.json; a schema-only PR must run Rust CI.
"""

import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
WORKFLOW = REPO_ROOT / ".github" / "workflows" / "rust-ci.yml"

try:
    import yaml  # type: ignore
except ImportError:
    yaml = None  # type: ignore


def load_workflow() -> dict:
    if yaml is None:
        raise unittest.SkipTest("PyYAML not available")
    with WORKFLOW.open(encoding="utf-8") as fh:
        data = yaml.safe_load(fh)
    # PyYAML 1.1 interprets the `on` key as boolean True; normalize it.
    if "on" not in data and True in data:
        data["on"] = data.pop(True)
    return data


class WorkflowPathFilterAuditTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.workflow = load_workflow()

    def test_schema_canonical_input_in_pull_request_paths(self) -> None:
        paths = self.workflow["on"]["pull_request"]["paths"]
        self.assertIn("docs/recipe-schema-v1.json", paths)

    def test_schema_canonical_input_in_push_paths(self) -> None:
        paths = self.workflow["on"]["push"]["paths"]
        self.assertIn("docs/recipe-schema-v1.json", paths)

    def test_rust_tree_in_pull_request_paths(self) -> None:
        paths = self.workflow["on"]["pull_request"]["paths"]
        self.assertIn("rust/**", paths)

    def test_workflow_self_in_paths(self) -> None:
        pr_paths = self.workflow["on"]["pull_request"]["paths"]
        push_paths = self.workflow["on"]["push"]["paths"]
        self.assertIn(".github/workflows/rust-ci.yml", pr_paths)
        self.assertIn(".github/workflows/rust-ci.yml", push_paths)

    def test_no_wildcard_dependency_floor_in_workflow(self) -> None:
        # Regression guard for finding 1: the coverage step must not contain
        # a fixed --fail-under percentage floor.
        coverage_steps = []
        for job in self.workflow["jobs"].values():
            for step in job.get("steps", []):
                run = step.get("run") or ""
                if "check_coverage_regression.py" in run:
                    coverage_steps.append(run)
        self.assertTrue(coverage_steps, "expected a check_coverage_regression.py step")
        for run in coverage_steps:
            self.assertNotIn("--fail-under", run)


if __name__ == "__main__":
    unittest.main()
