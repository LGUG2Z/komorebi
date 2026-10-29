"""A suite as a list: shared scenarios, and what a platform adds to them.

A suite file says what runs, in order, and each step labels the checks it makes: `shared` for a
scenario that runs on every platform, and a platform's own name for what only that platform can
check. The output reads `[shared] ...` and `[kde] ...`, and the same labels say, for each check,
which port it belongs to.

Each step prints the time it took, so that a log says which scenario the time went to.

To run only some of the steps, set `KFDE_ONLY` to a comma-separated list: a step runs when its
name (`module.function`, as printed) contains one of the words, and a step made with `always=True`
(what the others depend on, such as the one that opens the windows) runs whatever the list says.
The steps that do not run are printed, and return `None`.
"""

import os
import time
from collections.abc import Callable
from dataclasses import dataclass

from komorebi_vm.core.checks import Checks

ONLY = "KFDE_ONLY"


@dataclass(frozen=True)
class Step:
    """A thing to run, and the scope of the checks it makes."""

    scope: str
    run: Callable[[], object]
    name: str = ""
    always: bool = False


def _name(fn: Callable[..., object]) -> str:
    """`module.function` of a scenario or an extra, for the line that says what runs."""
    module = getattr(fn, "__module__", "").rsplit(".", 1)[-1]
    return f"{module}.{getattr(fn, '__name__', '')}".strip(".")


def shared(scenario: Callable[..., object], *args: object, always: bool = False) -> Step:
    """A scenario of `komorebi_vm.core.scenarios`, called with `args`."""
    return Step("shared", lambda: scenario(*args), _name(scenario), always)


def platform_only(
    scope: str, extra: Callable[..., object], *args: object, always: bool = False
) -> Step:
    """What one platform adds: an extra of that platform, called with `args`, under its name."""
    return Step(scope, lambda: extra(*args), _name(extra), always)


def _wanted() -> list[str]:
    return [word.strip() for word in os.environ.get(ONLY, "").split(",") if word.strip()]


def run_suite(checks: Checks, *steps: Step) -> list[object]:
    """Runs the steps in order, each with its scope on `checks`, and returns what each returned,
    so that a later part of a suite can use the windows a scenario opened.

    A step that `KFDE_ONLY` leaves out is not run. A step that is run prints how long it took."""
    wanted = _wanted()
    results: list[object] = []
    for step in steps:
        if wanted and not step.always and not any(word in step.name for word in wanted):
            print(f"== [{step.scope}] {step.name}: left out by {ONLY}")
            results.append(None)
            continue
        if step.name:
            print(f"== [{step.scope}] {step.name}")
        started = time.monotonic()
        with checks.scoped(step.scope):
            results.append(step.run())
        if step.name:
            print(f"   [{step.scope}] {step.name}: {time.monotonic() - started:.1f} s")
    return results
