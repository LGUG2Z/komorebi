"""A paused daemon leaves new windows alone, and an unpaused one takes them up."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled

# how long a daemon that is paused has to leave a window alone for it to count
QUIET = 3


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Pauses the daemon, opens a window, unpauses it and opens another, with `clients` tiled
    on the first workspace. Closes both."""
    managed = platform.state().managed()

    platform.komorebic("toggle-pause")
    paused = platform.spawn("while-paused")
    checks.check(
        "a window that opens while the daemon is paused is not managed",
        not platform.wait_for(lambda: platform.state().managed() > managed, timeout=QUIET),
    )

    platform.komorebic("toggle-pause")
    after = platform.spawn("after-the-pause")
    checks.check(
        "after 'toggle-pause' again the next window is managed",
        platform.wait_for(lambda: platform.state().managed() > managed, timeout=20),
    )
    checks.check(
        "... and every window it had is still tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, [*clients, after]), timeout=20),
    )

    platform.close(paused)
    platform.close(after)
    checks.check(
        "the two windows are closed and the daemon has the ones it had",
        platform.wait_for(lambda: platform.state().managed() == managed, timeout=20),
    )
