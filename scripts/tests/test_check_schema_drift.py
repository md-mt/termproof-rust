"""Unit tests for the Rust workspace schema-drift gate.

The schema-drift gate (RUST-003, issue #96) protects the canonical recipe
schema from silently drifting: the canonical Draft 2020-12 schema lives at
docs/recipe-schema-v1.json, and any schema copy checked into the Rust
workspace must stay byte-identical to it. The gate also fails fast when the
canonical file stops being a valid JSON document or stops declaring Draft
2020-12, so a corrupt/renamed schema cannot pass unnoticed.
"""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / "rust" / "scripts" / "check_schema_drift.py"


def run_script(*args: str, cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args],
        cwd=cwd,
        capture_output=True,
        text=True,
    )


class CanonicalSchemaTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)

    def write_canonical(self, schema: dict) -> Path:
        path = self.root / "docs" / "recipe-schema-v1.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(schema), encoding="utf-8")
        (self.root / "rust").mkdir(exist_ok=True)
        return path

    def test_valid_draft2020_12_canonical_passes(self) -> None:
        canonical = self.write_canonical(
            {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": "https://example.test/recipe-schema-v1.json",
                "type": "object",
            }
        )
        result = run_script(
            "--canonical", str(canonical), "--rust-root", str(self.root / "rust"),
            cwd=self.root,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_schema_field_fails(self) -> None:
        canonical = self.write_canonical({"type": "object"})
        result = run_script(
            "--canonical", str(canonical), "--rust-root", str(self.root / "rust"),
            cwd=self.root,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("must declare $schema", result.stderr)

    def test_invalid_json_fails(self) -> None:
        canonical = self.root / "docs" / "recipe-schema-v1.json"
        canonical.parent.mkdir(parents=True, exist_ok=True)
        canonical.write_text("{ not json", encoding="utf-8")
        result = run_script(
            "--canonical", str(canonical), "--rust-root", str(self.root / "rust"),
            cwd=self.root,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not valid JSON", result.stderr)

    def test_missing_canonical_fails(self) -> None:
        missing = self.root / "docs" / "recipe-schema-v1.json"
        result = run_script(
            "--canonical", str(missing), "--rust-root", str(self.root / "rust"),
            cwd=self.root,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("missing", result.stderr)


class RustCopyDriftTest(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.root = Path(self._tmp.name)
        self.canonical = self.root / "docs" / "recipe-schema-v1.json"
        self.canonical.parent.mkdir(parents=True, exist_ok=True)
        self.schema = {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "https://github.com/md-mt/termproof/docs/recipe-schema-v1.json",
            "title": "TermProof recipe v1",
            "type": "object",
            "required": ["name", "command"],
        }
        self.canonical.write_text(json.dumps(self.schema), encoding="utf-8")
        self.rust_root = self.root / "rust"
        self.rust_root.mkdir(parents=True)

    def make_copy(self, rel: str, content: str) -> Path:
        path = self.rust_root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def run_gate(self, *extra: str) -> subprocess.CompletedProcess:
        return run_script(
            "--canonical", str(self.canonical), "--rust-root", str(self.rust_root),
            *extra, cwd=self.root,
        )

    def test_identical_rust_copy_passes(self) -> None:
        self.make_copy("crates/termproof-core/resources/recipe-schema-v1.json",
                       json.dumps(self.schema))
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_drifted_rust_copy_fails(self) -> None:
        drifted = dict(self.schema)
        drifted["title"] = "TermProof recipe v1 (drifted)"
        self.make_copy("crates/termproof-core/resources/recipe-schema-v1.json",
                       json.dumps(drifted))
        result = self.run_gate()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("drift", result.stderr)

    def test_build_artifacts_are_ignored(self) -> None:
        # target/ build output must never be treated as a schema copy.
        self.make_copy("target/debug/recipe-schema-v1.json",
                       json.dumps({"$schema": "https://json-schema.org/draft/2020-12/schema"}))
        result = self.run_gate()
        self.assertEqual(result.returncode, 0, result.stderr)


class CLIInvocationTest(unittest.TestCase):
    def test_script_requires_canonical(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            result = run_script("--rust-root", tmp, cwd=Path(tmp))
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("--canonical", result.stderr)


if __name__ == "__main__":
    unittest.main()
