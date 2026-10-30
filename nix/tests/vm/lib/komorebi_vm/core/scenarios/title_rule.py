"""A rule decides when a window opens: a title change later does not make the daemon look again."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import QUIET

PENDING = "pending-rule"
RENAMED = "renamed-rule"
MANAGED = "managed-before"
MANAGED_RENAMED = "managed-after"
AFTER = 6


def run(platform: Platform, checks: Checks) -> None:
    """A window that opens with a title an ignore rule matches is left alone, and stays so when it
    gives itself another title; a window that is managed stays managed when it renames itself.

    The daemon runs with its windows tiled; the windows are closed at the end, and the rule stays.
    """
    platform.komorebic("ignore-rule", "title", PENDING)
    before = platform.state().managed()

    platform.spawn(PENDING, rename_to=RENAMED, after=AFTER)
    renamed = Client(RENAMED)
    checks.check(
        "a window the ignore rule matches by its title is left alone",
        platform.state().workspace_of(PENDING) is None and platform.state().managed() == before,
    )
    checks.check(
        "it gives itself another title",
        platform.wait_for(lambda: platform.os_window(renamed) is not None, timeout=30),
    )
    checks.check(
        "... and the daemon leaves it alone: a rule decides when a window opens",
        not platform.wait_for(lambda: platform.state().managed() != before, timeout=QUIET),
    )
    platform.close(renamed)
    platform.wait_for(lambda: platform.os_window(renamed) is None, timeout=10)

    platform.spawn(MANAGED, rename_to=MANAGED_RENAMED, after=AFTER)
    kept = Client(MANAGED_RENAMED)
    checks.check(
        "a window that is managed when it renames itself stays managed",
        platform.wait_for(lambda: platform.os_window(kept) is not None, timeout=30)
        and platform.wait_for(lambda: platform.state().managed() == before + 1, timeout=10),
    )
    platform.close(kept)
    checks.check(
        "closed, it leaves the daemon",
        platform.wait_for(
            lambda: platform.os_window(kept) is None and platform.state().managed() == before,
            timeout=10,
        ),
    )
