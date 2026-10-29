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
        self.failures: list[str] = []
        self.skipped: list[str] = []

    def ok(self, description: str) -> None:
        """Records a check that passed."""
        self.passed += 1
        print(f"  ok   {description}")

    def bad(self, description: str) -> None:
        """Records a check that failed."""
        self.failed += 1
        self.failures.append(description)
        print(f"  FAIL {description}")

    def skip(self, description: str, reason: str) -> None:
        """Records a check that was not made, and why: the platform cannot do what it asks."""
        self.skipped.append(f"{description} ({reason})")
        print(f"  skip {description}: {reason}")

    def check(self, description: str, condition: bool | Callable[[], bool]) -> bool:
        """Records `condition` (or what calling it returns) and returns it."""
        holds = condition() if callable(condition) else condition
        if holds:
            self.ok(description)
        else:
            self.bad(description)
        return holds

    def finish(self) -> None:
        """Prints `passed: N failed: M` and raises if any check failed.

        The error names the checks that failed, so that the last lines of a failed run, which
        are all `nix build` shows, say which they were.
        """
        print(f"passed: {self.passed} failed: {self.failed}")
        if self.skipped:
            print(f"skipped: {len(self.skipped)}")
        if self.failed:
            raise AssertionError(f"{self.failed} checks failed: {'; '.join(self.failures)}")
