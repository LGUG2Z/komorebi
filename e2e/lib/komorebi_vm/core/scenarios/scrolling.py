"""In the scrolling layout the window that has the focus is in view, wherever the others are."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Changes the layout of the first workspace, which has `clients` tiled in a container each,
    to scrolling, gives each of them the focus in turn, and changes it back to bsp."""
    area = platform.work_area()

    def in_view(client: Client) -> bool:
        window = platform.os_window(client)
        return window is not None and window.frame.inside(area)

    platform.komorebic("change-layout", "scrolling")
    for client in clients:
        platform.activate(client)
        checks.check(
            f"in scrolling, {client.title} has the focus and is inside the work area",
            platform.wait_for(
                lambda c=client: focus_agrees(platform, clients, c.title) and in_view(c),
                timeout=20,
            ),
        )

    platform.komorebic("change-layout", "bsp")
    checks.check(
        "back in bsp every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
