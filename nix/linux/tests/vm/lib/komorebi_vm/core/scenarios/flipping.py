"""A flipped layout is the layout mirrored, and flipping it back gives the layout back."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Flips the layout of the first workspace, which has `clients` tiled in a container each,
    horizontally and back."""
    area = platform.work_area()

    def centres() -> dict[str, int]:
        found = {}
        for client in clients:
            window = platform.os_window(client)
            if window is not None:
                found[client.title] = window.frame.x + window.frame.w // 2
        return found

    before = centres()

    def mirrored() -> bool:
        now = centres()
        return len(now) == len(before) and all(
            abs((2 * area.x + area.w - before[t]) - now[t]) <= platform.tolerance for t in before
        )

    platform.komorebic("flip-layout", "horizontal")
    checks.check(
        "after 'flip-layout horizontal' every window is where the mirror of its place is",
        platform.wait_for(mirrored, timeout=20),
    )
    checks.check(
        "... and every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )

    platform.komorebic("flip-layout", "horizontal")
    checks.check(
        "flipped again, every window is where it was",
        platform.wait_for(
            lambda: all(abs(centres()[t] - before[t]) <= platform.tolerance for t in before),
            timeout=20,
        ),
    )
