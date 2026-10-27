"""Counted checks, so that one failure does not hide the ones after it."""

from collections.abc import Callable


class Checks:
    """Collects the outcome of every check and fails once, at the end.

    A suite is a long scenario: a failure in the middle should not stop the
    rest from running, because the rest says what else is broken. `finish`
    prints the totals and raises if any check failed.
    """

    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0

    def ok(self, description: str) -> None:
        """Records a check that passed."""
        self.passed += 1
        print(f"  ok   {description}")

    def bad(self, description: str) -> None:
        """Records a check that failed."""
        self.failed += 1
        print(f"  FAIL {description}")

    def check(self, description: str, condition: bool | Callable[[], bool]) -> bool:
        """Records `condition` (or what calling it returns) and returns it."""
        holds = condition() if callable(condition) else condition
        if holds:
            self.ok(description)
        else:
            self.bad(description)
        return holds

    def finish(self) -> None:
        """Prints `passed: N failed: M` and raises if any check failed."""
        print(f"passed: {self.passed} failed: {self.failed}")
        if self.failed:
            raise AssertionError(f"{self.failed} checks failed")
