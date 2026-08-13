#!/usr/bin/env python3
"""Record the Python implementation's assertion behaviour as a differential corpus.

This is the oracle half of the assertion differential harness, the sibling of
``probe_steps.py``. It drives the eight Python built-in assertions over
``harness/corpus/assertion_cases.json`` and writes each case's observed
``name``/``passed``/``detail`` to ``harness/corpus/assertions.expected.json``.
The Rust half (``crates/termproof-core/tests/differential_assertions.rs``)
replays the same corpus and reports the agreement count, so the measurement can
be reproduced without a Python interpreter.

Usage::

    TERMPROOF_PYTHON_REPO=/path/to/python/termproof \\
        python3 harness/probe_assertions.py \\
        > harness/corpus/assertions.expected.json

The Python checkout must have its dependencies importable (``uv run`` from that
checkout is the usual way). The recorded environment is written into the corpus
header because several details are CPython- and ``jsonschema``-version
dependent — see `specs/003-builtin-assertions/spec.md` FR-016 and FR-017.

Two case kinds appear in the corpus:

``assertion``
    One assertion evaluated on its own. Records what the runner's per-assertion
    dispatch produced, or — for the inputs that make the oracle raise — the
    exception that ended the run.

``run``
    A whole recipe's assertion list, transcribed from
    ``TermProofRunner.evaluate_assertions``. Records the evaluated list in
    order, the score and the overall verdict, so FR-019 ordering and FR-022
    scoring are measured rather than assumed.

Every case runs against a real fixture tree in a temporary directory. Absolute
paths appear in details (`file_exists`, `schema file unreadable:`), so the
fixture root is substituted back out as ``@FX`` before recording — the
`specs/OBSERVATION-LOG.md` §4 constraint that makes an absolute path portable.
"""

from __future__ import annotations

import importlib.metadata
import json
import os
import platform
import sys
import tempfile
from pathlib import Path
from typing import Any

_REPO = os.environ.get("TERMPROOF_PYTHON_REPO")
if not _REPO:
    sys.exit("TERMPROOF_PYTHON_REPO must point at a Python TermProof checkout")
sys.path.insert(0, _REPO)

from termproof import builtin_assertions  # noqa: E402
from termproof.models import CommandSpec, Recipe, score_from_assertions  # noqa: E402

# termproof/config.py DEFAULT_CONFIG["assertions"], transcribed. The runner
# builds this registry from config; the corpus only ever uses the defaults.
ASSERTIONS = {
    "output_contains": builtin_assertions.OutputContains,
    "output_not_contains": builtin_assertions.OutputNotContains,
    "screen_contains": builtin_assertions.ScreenContains,
    "screen_not_contains": builtin_assertions.ScreenNotContains,
    "exit_code": builtin_assertions.ExitCode,
    "file_exists": builtin_assertions.FileExists,
    "file_contains": builtin_assertions.FileContains,
    "json_schema": builtin_assertions.JsonSchema,
}

FIXTURE_TOKEN = "@FX"

DEFAULT_SCREEN = "SCREEN text"
DEFAULT_RAW = "RAW output"


def build_fixtures(root: Path, spec: dict[str, Any]) -> None:
    """Materialise the corpus's fixture tree under ``root``.

    ``None`` means a directory; ``@hex:...`` means those raw bytes, which is how
    a file that is not valid UTF-8 gets into a JSON corpus.
    """
    for relative, content in spec.items():
        target = root / relative
        if content is None:
            target.mkdir(parents=True, exist_ok=True)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, str) and content.startswith("@hex:"):
            target.write_bytes(bytes.fromhex(content.removeprefix("@hex:")))
        else:
            target.write_text(content, encoding="utf-8")


def substitute(value: Any, root: str) -> Any:
    """Replace ``@FX`` with the fixture root throughout a case."""
    if isinstance(value, str):
        return value.replace(FIXTURE_TOKEN, root)
    if isinstance(value, dict):
        return {k: substitute(v, root) for k, v in value.items()}
    if isinstance(value, list):
        return [substitute(v, root) for v in value]
    return value


def redact(text: str, root: str) -> str:
    """Put ``@FX`` back, so a recorded detail is comparable across machines."""
    return text.replace(root, FIXTURE_TOKEN)


def recipe_for(case: dict[str, Any], root: str) -> Recipe:
    cwd = case.get("cwd", FIXTURE_TOKEN)
    if cwd is not None:
        cwd = substitute(cwd, root)
    return Recipe(
        name=case["id"],
        command=CommandSpec(argv=["true"], cwd=cwd),
        steps=[],
        assertions=substitute(case.get("assertions", []), root),
        expect_exit_code=case.get("expect_exit_code", 0),
    )


def evaluate_one(
    recipe: Recipe,
    assertion: dict[str, Any],
    screen: str,
    raw_output: str,
    exit_code: int | None,
) -> Any:
    """``TermProofRunner._evaluate_assertion``, transcribed.

    Both raising paths are the runner's own: ``assertion["type"]`` for a missing
    key, and the ``ValueError`` it wraps the registry's ``KeyError`` in.
    """
    kind = assertion["type"]
    evaluator_cls = ASSERTIONS.get(kind)
    if evaluator_cls is None:
        raise ValueError(f"unknown assertion type: {kind}")
    return evaluator_cls().evaluate(recipe, assertion, screen, raw_output, exit_code)


def aborted(exc: BaseException) -> dict[str, Any]:
    return {"aborts": True, "exception": type(exc).__name__, "message": str(exc)}


def run_assertion_case(case: dict[str, Any], root: str) -> dict[str, Any]:
    recipe = recipe_for(case, root)
    assertion = substitute(case["assertion"], root)
    screen = case.get("screen", DEFAULT_SCREEN)
    raw_output = case.get("raw_output", DEFAULT_RAW)
    exit_code = case.get("exit_code", 0)
    try:
        result = evaluate_one(recipe, assertion, screen, raw_output, exit_code)
    except Exception as exc:  # noqa: BLE001 — the oracle contains nothing
        return aborted(exc)
    return {
        "name": redact(result.name, root),
        "passed": result.passed,
        "detail": redact(result.detail, root),
    }


def run_run_case(case: dict[str, Any], root: str) -> dict[str, Any]:
    """``TermProofRunner.evaluate_assertions``, transcribed (runner.py:310-323)."""
    recipe = recipe_for(case, root)
    screen = case.get("screen", DEFAULT_SCREEN)
    raw_output = case.get("raw_output", DEFAULT_RAW)
    exit_code = case.get("exit_code", 0)

    assertions = list(recipe.assertions)
    if recipe.expect_exit_code is not None:
        assertions.append({"type": "exit_code", "value": recipe.expect_exit_code})
    try:
        results = [
            evaluate_one(recipe, assertion, screen, raw_output, exit_code)
            for assertion in assertions
        ]
    except Exception as exc:  # noqa: BLE001 — one bad assertion loses them all
        return aborted(exc)
    return {
        "results": [
            {
                "name": redact(r.name, root),
                "passed": r.passed,
                "detail": redact(r.detail, root),
            }
            for r in results
        ],
        "score": score_from_assertions(results),
        "passed": all(r.passed for r in results),
    }


def main() -> int:
    corpus_path = Path(__file__).resolve().parent / "corpus" / "assertion_cases.json"
    corpus = json.loads(corpus_path.read_text(encoding="utf-8"))

    recorded = []
    with tempfile.TemporaryDirectory(prefix="termproof-assert-") as tmp:
        # macOS hands out /var/... which is a symlink to /private/var. Python's
        # Path does not resolve it and neither should the port, but the recorded
        # detail has to be redacted against the string the case actually used.
        root = tmp
        build_fixtures(Path(root), corpus["fixtures"])
        for case in corpus["cases"]:
            if case.get("kind", "assertion") == "run":
                observed = run_run_case(case, root)
            else:
                observed = run_assertion_case(case, root)
            entry = dict(case)
            entry["expected"] = observed
            recorded.append(entry)

    document = {
        "_comment": (
            "Generated by harness/probe_assertions.py against the Python "
            "implementation. Do not hand-edit; regenerate instead."
        ),
        "environment": {
            "python": platform.python_version(),
            "platform": platform.system(),
            "jsonschema": importlib.metadata.version("jsonschema"),
        },
        "fixtures": corpus["fixtures"],
        "cases": recorded,
    }
    json.dump(document, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
