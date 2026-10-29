"""A rule that matches a title decides when the window has its title."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform

PENDING = "pending-rule"
RENAMED = "renamed-rule"


def run(platform: Platform, checks: Checks) -> None:
    """A window that opens with a title the ignore rule matches is left alone, and tiled once it
    gives itself another title.

    The daemon runs with its windows tiled; the window is closed at the end, and the rule stays.
    """
    platform.komorebic("ignore-rule", "title", PENDING)
    before = platform.state().managed()

    # the window renames itself after the daemon has had time to decide about the first title
    platform.spawn(PENDING, rename_to=RENAMED, after=6)
    renamed = Client(RENAMED)
    checks.check(
        "a window the ignore rule matches by its title is left alone",
        not platform.wait_for(lambda: platform.state().managed() != before, timeout=2)
        and platform.os_window(Client(PENDING)) is not None,
    )
    checks.check(
        "once it has a title the rule does not match, it is tiled",
        platform.wait_for(
            lambda: (
                platform.os_window(renamed) is not None and platform.state().managed() == before + 1
            ),
            timeout=30,
        ),
    )
    window = platform.os_window(renamed)
    checks.check(
        "... and inside the work area",
        window is not None and window.frame.inside(platform.work_area(), 2),
    )
    platform.close(renamed)
    checks.check(
        "closed, it leaves the daemon",
        platform.wait_for(
            lambda: platform.os_window(renamed) is None and platform.state().managed() == before,
            timeout=10,
        ),
    )
