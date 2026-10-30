"""A workspace with tiling off leaves its windows where they are, and tiling it again tiles them."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import positions


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Toggles tiling off on the first workspace, which has `clients` tiled in a container each,
    and on again, with a layout change in between that must not move anything."""
    before = positions(platform, clients)

    platform.komorebic("toggle-tiling")
    checks.check(
        "after 'toggle-tiling' the daemon has the workspace untiled",
        platform.wait_for(lambda: not platform.state().workspace().tile, timeout=20),
    )
    checks.check("... and no window moved", positions(platform, clients) == before)
    platform.komorebic("change-layout", "columns")
    checks.check(
        "... a layout change does not move them either",
        not platform.wait_for(lambda: positions(platform, clients) != before, timeout=3),
    )

    platform.komorebic("toggle-tiling")
    checks.check(
        "'toggle-tiling' again has the workspace tiled",
        platform.wait_for(lambda: platform.state().workspace().tile, timeout=20),
    )
    checks.check(
        "... and every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    platform.komorebic("change-layout", "bsp")
    checks.check(
        "... and in bsp again",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
