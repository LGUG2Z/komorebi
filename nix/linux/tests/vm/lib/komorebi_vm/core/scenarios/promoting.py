"""A promoted window takes the first tile, and keeps the focus."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Promotes the last of `clients`, which are tiled windows of the first workspace in a
    container each, with more than one of them."""
    target = clients[-1]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))
    first = platform.state().workspace().latest_layout[0]

    platform.komorebic("promote")
    checks.check(
        f"after 'promote' the daemon has {target.title} in the first container",
        platform.wait_for(lambda: platform.state().containers()[0] == [target.title], timeout=20),
    )

    def on_the_first_tile() -> bool:
        window = platform.os_window(target)
        return window is not None and (window.frame.x, window.frame.y) == (first.x, first.y)

    checks.check("... and the window is on the first tile", platform.wait_for(on_the_first_tile))
    checks.check(
        "... every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    checks.check(
        "... and the window still has the focus",
        platform.wait_for(lambda: focus_agrees(platform, clients, target.title), timeout=20),
    )
