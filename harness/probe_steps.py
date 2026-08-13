#!/usr/bin/env python3
"""Record the Python implementation's step behaviour as a differential corpus.

This is the oracle half of the cross-runtime differential harness. It drives the
Python built-in steps over a fixed corpus and writes each case's observed
``passed``/``detail`` to ``harness/corpus/steps.json``. The Rust half
(``crates/termproof-core/tests/differential_steps.rs``) replays the same corpus
through the Rust steps and reports the agreement count, so the measurement can
be reproduced without a Python interpreter.

Usage::

    TERMPROOF_PYTHON_REPO=/path/to/python/termproof \\
        python3 harness/probe_steps.py > harness/corpus/steps.json

The Python checkout must have its dependencies importable (``uv run`` from that
checkout is the usual way). The recorded environment is written into the corpus
header, because several details are CPython-, libc- and ``ptyprocess``-version
dependent — see `specs/002-builtin-steps/spec.md` FR-004, FR-008 and FR-016.

Two session kinds appear in the corpus:

``stub``
    A session with pre-seeded ``screen``/``raw_output`` and the wait loops
    transcribed from ``termproof/session.py``. Used for the waiting steps, where
    the case is about argument handling rather than terminal fidelity.

``child``
    A real ``pexpect`` child. Required for ``send_text``/``send_line``/``press``:
    a stub that appends to a list records ``send_text {"text": 5}`` as *passing*,
    which is the mistake spec 002 FR-022 calls out.
"""

from __future__ import annotations

import json
import math
import os
import platform
import sys
import time
from pathlib import Path
from typing import Any

_REPO = os.environ.get("TERMPROOF_PYTHON_REPO")
if not _REPO:
    sys.exit("TERMPROOF_PYTHON_REPO must point at a Python TermProof checkout")
sys.path.insert(0, _REPO)

import pexpect  # noqa: E402
import pyte  # noqa: E402

from termproof import builtin_steps  # noqa: E402

# Non-finite values cannot be written as JSON numbers, so the corpus spells them
# as these sentinel strings and both halves of the harness expand them.
SENTINELS = {"@nan": math.nan, "@inf": math.inf, "@-inf": -math.inf}


def expand(value: Any) -> Any:
    """Replace ``@nan``/``@inf``/``@-inf`` sentinels with the float they name."""
    if isinstance(value, str) and value in SENTINELS:
        return SENTINELS[value]
    if isinstance(value, dict):
        return {k: expand(v) for k, v in value.items()}
    if isinstance(value, list):
        return [expand(v) for v in value]
    return value


class StubSession:
    """Fixed-content session with the wait loops from ``termproof/session.py``.

    ``read_available`` is a no-op because the content never changes, but the
    loops still consult the wall clock, so timeout semantics — including a
    non-positive or NaN timeout exiting the loop immediately — are preserved.
    """

    def __init__(self, screen: str = "", raw: str = "", alive: bool = True) -> None:
        self.screen = screen
        self.raw_output = raw
        self._alive = alive
        self.exit_code: int | None = None
        self.log: list[str] = []

    def read_available(self, timeout: float = 0.0) -> None:
        return None

    def is_alive(self) -> bool:
        return self._alive

    def send_text(self, text: str) -> None:
        self.log.append(f"send_text:{text}")

    def send_line(self, text: str) -> None:
        self.send_text(text + "\r")

    def wait_for_text(self, text: str, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        while time.monotonic() < deadline:
            self.read_available(0.05)
            if text in self.screen or text in self.raw_output:
                return True
            if not self.is_alive():
                self.read_available(0)
                return text in self.screen or text in self.raw_output
        return False

    def wait_for_idle(self, stable_seconds: float, timeout_seconds: float) -> bool:
        deadline = time.monotonic() + timeout_seconds
        last_screen = self.screen
        last_raw_len = len(self.raw_output)
        stable_since: float | None = time.monotonic() if self.raw_output else None
        while time.monotonic() < deadline:
            self.read_available(0.05)
            current = self.screen
            raw_len = len(self.raw_output)
            if current != last_screen or (stable_since is None and raw_len != last_raw_len):
                last_screen = current
                last_raw_len = raw_len
                stable_since = time.monotonic()
            if stable_since is not None and time.monotonic() - stable_since >= stable_seconds:
                return True
            if not self.is_alive():
                self.read_available(0)
                return True
        return False


class ChildSession:
    """A real ``pexpect`` child, rendered through ``pyte`` as the product does."""

    def __init__(self, argv: list[str]) -> None:
        self.raw_output = ""
        self.exit_code: int | None = None
        self._pyte = pyte.Screen(80, 24)
        self._stream = pyte.Stream(self._pyte)
        self.child = pexpect.spawn(
            argv[0], argv[1:], encoding="utf-8", codec_errors="replace", timeout=1
        )

    @property
    def screen(self) -> str:
        return "\n".join(line.rstrip() for line in self._pyte.display).rstrip()

    def read_available(self, timeout: float = 0.0) -> None:
        try:
            chunk = self.child.read_nonblocking(4096, timeout=timeout)
        except Exception:
            return
        self.raw_output += chunk
        self._stream.feed(chunk)

    def is_alive(self) -> bool:
        return bool(self.child.isalive())

    def send_text(self, text: str) -> None:
        self.child.send(text)

    def send_line(self, text: str) -> None:
        self.send_text(text + "\r")

    def press(self, key: str) -> None:
        keys = {
            "enter": "\r",
            "escape": "\x1b",
            "tab": "\t",
            "backspace": "\x7f",
            "up": "\x1b[A",
            "down": "\x1b[B",
            "right": "\x1b[C",
            "left": "\x1b[D",
        }
        normalized = key.lower()
        if normalized.startswith("ctrl-"):
            self.child.sendcontrol(normalized.removeprefix("ctrl-"))
            return
        self.child.send(keys[normalized])

    def close(self) -> None:
        try:
            self.child.close(force=True)
        except Exception:
            pass


ACTIONS = {
    "wait_for_text": builtin_steps.WaitForText(),
    "wait_for_idle": builtin_steps.WaitForIdle(),
    "send_text": builtin_steps.SendText(),
    "send_line": builtin_steps.SendLine(),
    "press": builtin_steps.Press(),
    "sleep": builtin_steps.Sleep(),
    "wait_for_regex": builtin_steps.WaitForRegex(),
}


def run_case(case: dict[str, Any]) -> dict[str, Any]:
    """Execute one case, reproducing the runner's exception containment."""
    step = expand(case["step"])
    session_spec = case.get("session", {})
    kind = session_spec.get("kind", "stub")
    if kind == "child":
        session: Any = ChildSession(session_spec.get("argv", ["cat"]))
        if session_spec.get("settle"):
            session.read_available(0.2)
    else:
        session = StubSession(
            session_spec.get("screen", ""),
            session_spec.get("raw", ""),
            session_spec.get("alive", True),
        )
    index = case.get("index", 1)
    try:
        action_name = step["action"]
        action = ACTIONS.get(action_name)
        if action is None:
            raise ValueError(f"unknown step action: {action_name}")
        result = action.execute(session, step, index)
        observed = {"name": result.name, "passed": result.passed, "detail": result.detail}
    except Exception as exc:  # noqa: BLE001 — the runner catches everything
        # termproof/runner.py:243-247 — any exception becomes a failed StepResult
        # whose detail is str(exc).
        name = step.get("name", f"{index}:{step.get('action')}")
        observed = {"name": name, "passed": False, "detail": str(exc)}
    finally:
        if kind == "child":
            session.close()
    return observed


def main() -> int:
    corpus_path = Path(__file__).resolve().parent / "corpus" / "cases.json"
    cases = json.loads(corpus_path.read_text(encoding="utf-8"))
    recorded = []
    for case in cases:
        observed = run_case(case)
        entry = dict(case)
        entry["expected"] = observed
        recorded.append(entry)
    document = {
        "_comment": (
            "Generated by harness/probe_steps.py against the Python implementation. "
            "Do not hand-edit; regenerate instead."
        ),
        "environment": {
            "python": platform.python_version(),
            "platform": platform.system(),
            "pexpect": getattr(pexpect, "__version__", "unknown"),
            "pyte": getattr(pyte, "__version__", "unknown"),
        },
        "cases": recorded,
    }
    json.dump(document, sys.stdout, indent=2, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
