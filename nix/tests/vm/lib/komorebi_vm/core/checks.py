"""Counted checks, so that one failure does not hide the ones after it."""

from collections.abc import Callable, Generator
from contextlib import contextmanager


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
        self.known: list[str] = []
        self.scope = ""
        """Where the checks being made come from (`shared`, `kde`, `windows`), or empty. It prefixes
        the description of each check, and a subclass that records checks may read it."""

    @contextmanager
    def scoped(self, scope: str) -> Generator[None]:
        """Makes the checks inside the block those of `scope`, and puts the scope before it back
        after, so that scopes nest."""
        before, self.scope = self.scope, scope
        try:
            yield
        finally:
            self.scope = before

    def labelled(self, description: str) -> str:
        """The description with the scope in front of it, `[kde] ...`, when there is one."""
        return f"[{self.scope}] {description}" if self.scope else description

    def ok(self, description: str) -> None:
        """Records a check that passed."""
        self.passed += 1
        print(f"  ok   {self.labelled(description)}")

    def bad(self, description: str) -> None:
        """Records a check that failed."""
        self.failed += 1
        self.failures.append(self.labelled(description))
        print(f"  FAIL {self.labelled(description)}")

    def skip(self, description: str, reason: str) -> None:
        """Records a check that was not made, and why: the platform cannot do what it asks."""
        self.skipped.append(f"{self.labelled(description)} ({reason})")
        print(f"  skip {self.labelled(description)}: {reason}")

    def known_failure(self, description: str, condition: bool, reason: str) -> bool:
        """Records a check that is known to fail, with the reason, to follow up on later.

        The suite does not fail while it does. When it holds, that is a failure, to say that
        the marker can go.
        """
        if condition:
            self.bad(f"{description}: it holds now, so it is no longer a known failure")
        else:
            self.known.append(f"{self.labelled(description)} ({reason})")
            print(f"  known failure {self.labelled(description)}: {reason}")
        return condition

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
        if self.known:
            print(f"known failures: {len(self.known)}")
        if self.failed:
            raise AssertionError(f"{self.failed} checks failed: {'; '.join(self.failures)}")
