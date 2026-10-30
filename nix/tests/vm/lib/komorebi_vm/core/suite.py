"""A suite as a list: shared scenarios, and what a platform adds to them.

A suite file says what runs, in order, and each step labels the checks it makes: `shared` for a
scenario that runs on every platform, and a platform's own name for what only that platform can
check. The output reads `[shared] ...` and `[kde] ...`, and the same labels say, for each check,
which port it belongs to.
"""

from collections.abc import Callable
from dataclasses import dataclass

from komorebi_vm.core.checks import Checks


@dataclass(frozen=True)
class Step:
    """A thing to run, and the scope of the checks it makes."""

    scope: str
    run: Callable[[], object]


def shared(scenario: Callable[..., object], *args: object) -> Step:
    """A scenario of `komorebi_vm.core.scenarios`, called with `args`."""
    return Step("shared", lambda: scenario(*args))


def platform_only(scope: str, extra: Callable[..., object], *args: object) -> Step:
    """What one platform adds: an extra of that platform, called with `args`, under its name."""
    return Step(scope, lambda: extra(*args))


def run_suite(checks: Checks, *steps: Step) -> None:
    """Runs the steps in order, each with its scope on `checks`."""
    for step in steps:
        with checks.scoped(step.scope):
            step.run()
