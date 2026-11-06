"""What a runner of a port needs besides the scenarios: the checks it keeps for the result file,
the steps it runs one at a time with their timings, and the commit under test.

Nothing here knows a port. A port says which steps are known failures (and which commit fixed
what) to `run_steps`, and keeps its own tables.
"""

import os
import subprocess
import time
from collections.abc import Callable
from pathlib import Path

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.suite import run_suite


class RecordedChecks(Checks):
    """`Checks` that also keeps each check, for the result file."""

    def __init__(self, known_checks: dict[str, str] | None = None) -> None:
        super().__init__()
        self.results: list[dict[str, object]] = []
        # how long each step of the suite took, for the result file
        self.steps: list[dict[str, object]] = []
        # the reason the failing checks of the step being run are known ones
        self.expected: str | None = None
        # a part of the description of a check, and the reason it is a known failure of the port: a
        # finer mark than a step, for a scenario of which only some checks are about the difference
        self.known_checks = known_checks or {}
        # the marks whose check passed, which a run says can go
        self.known_passed: list[str] = []

    def _known_check(self, description: str) -> tuple[str, str] | None:
        for key, reason in self.known_checks.items():
            if key in description:
                return key, reason
        return None

    def stale_marks(self) -> None:
        """Fails once for each known-check mark whose check passed: the difference is gone."""
        for key in self.known_passed:
            description = f"known-failure mark no longer needed, its check passes: {key}"
            # not `self.bad`: this text holds the mark's own words, and would be taken for the
            # check it is about
            Checks.bad(self, description)
            self._record(check=description, passed=False)

    def _record(self, **fields: object) -> None:
        if self.scope:
            fields["scope"] = self.scope
        self.results.append(fields)

    def ok(self, description: str) -> None:
        super().ok(description)
        self._record(check=description, passed=True)
        if (mark := self._known_check(description)) and mark[0] not in self.known_passed:
            self.known_passed.append(mark[0])

    def bad(self, description: str) -> None:
        mark = self._known_check(description)
        reason = self.expected or (mark[1] if mark else None)
        if reason:
            self.known.append(f"{self.labelled(description)} ({reason})")
            print(f"  known failure {self.labelled(description)}: {reason}")
            self._record(check=description, known_failure=True, reason=reason)
            return
        super().bad(description)
        self._record(check=description, passed=False)

    def skip(self, description: str, reason: str) -> None:
        super().skip(description, reason)
        self._record(check=description, skipped=True, reason=reason)

    def known_failure(
        self, description: str, condition: bool, reason: str
    ) -> bool:
        holds = super().known_failure(description, condition, reason)
        if not holds:
            self._record(
                check=description, known_failure=True, reason=reason
            )
        return holds



def run_steps(
    checks: "RecordedChecks",
    *steps,
    known: dict[str, str] | None = None,
    fixed_by: dict[str, tuple[str, str | None]] | None = None,
    contains: Callable[[str], bool] | None = None,
):
    """`run_suite`, one step at a time, keeping how long each took.

    `known` maps a step to the reason its failing checks are known failures, and `fixed_by` maps a
    step to the commit that fixed it and the reason that still holds after it (`None`: nothing);
    `contains` says whether the commit under test has a fix."""
    known = known or {}
    fixed_by = fixed_by or {}
    results: list[object] = []
    for step in steps:
        began = time.monotonic()
        counted = checks.passed + checks.failed + len(checks.known)
        fixed = fixed_by.get(step.name)
        checks.expected = (
            fixed[1] if fixed and contains and contains(fixed[0])
            else known.get(step.name)
        )
        known_before = len(checks.known)
        try:
            results.extend(run_suite(checks, step))
        finally:
            reason, checks.expected = checks.expected, None
        ran = checks.passed + checks.failed + len(checks.known) > counted
        if reason and ran and len(checks.known) == known_before:
            checks.bad(
                f"{step.name}: nothing failed, so it is no longer a known "
                "failure"
            )
        checks.steps.append({
            "step": f"[{step.scope}] {step.name}",
            "seconds": round(time.monotonic() - began, 1),
        })
    return results


def with_kwargs(fn, **kwargs):
    """`fn` with keyword arguments, for a suite step, which takes positional
    ones only; it keeps the name the step prints."""
    def step(*args):
        return fn(*args, **kwargs)

    step.__module__ = fn.__module__
    step.__name__ = fn.__name__
    return step


def contains(fix: str, repo: Path | None = None) -> bool:
    """Whether the commit under test has the commit `fix` in its history, so that
    a marker for the bug it fixed no longer applies. Not known: no."""
    commit = os.environ.get("KOMOREBI_E2E_COMMIT")
    if not commit:
        return False
    here = repo or Path(__file__).resolve().parent
    git = subprocess.run(
        ["git", "-C", str(here), "merge-base", "--is-ancestor", fix, commit],
        capture_output=True, check=False,
    )
    return git.returncode == 0



def commit() -> str:
    """The kfw commit under test: set `KOMOREBI_E2E_COMMIT` to the commit the
    binaries were built from; otherwise this tree's HEAD, which may not be it."""
    if os.environ.get("KOMOREBI_E2E_COMMIT"):
        return os.environ["KOMOREBI_E2E_COMMIT"]
    here = Path(__file__).resolve().parent
    try:
        git = subprocess.run(
            ["git", "-C", str(here), "rev-parse", "HEAD"],
            capture_output=True, text=True, check=False,
        )
    except FileNotFoundError:
        return "unknown"
    return git.stdout.strip() or "unknown"
