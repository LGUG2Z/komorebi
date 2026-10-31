"""`minimize`, `unmanage` and `manage`: the daemon lets go of the focused window, and takes it up
again."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import describe, focus_agrees
from komorebi_vm.core.scenarios.tiling import _where_the_layout_says


def _on_a_tile(platform: Platform, client: Client) -> bool:
    """The window is where the daemon's layout puts one of its tiles."""
    window = platform.os_window(client)
    layout = platform.state().workspace().latest_layout
    return window is not None and any(
        _where_the_layout_says(window.frame, tile, platform.tolerance) for tile in layout
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Runs `minimize`, `unmanage` and `manage` on the window that has the focus, which is one of
    `clients`: tiled windows of the first workspace, in a container each.

    They take a window out of the daemon's hands, so a sweep of commands does not run them. The
    window is tiled again at the end.
    """
    title = platform.state().focused_title()
    target = Client(title)
    before = platform.state().managed()
    platform.wait_for(lambda: focus_agrees(platform, clients, title), timeout=10)

    def has_let_go() -> bool:
        return platform.state().managed() == before - 1 and platform.os_window(target) is not None

    def has_it() -> bool:
        return platform.state().managed() == before and _on_a_tile(platform, target)

    platform.komorebic("minimize")
    checks.check(
        f"minimize: the daemon lets go of {title}, which stays open",
        platform.wait_for(has_let_go, timeout=10),
    )
    platform.activate(target)
    checks.check(f"{title}, restored, is tiled again", platform.wait_for(has_it, timeout=10))

    platform.wait_for(lambda: focus_agrees(platform, clients, title), timeout=10)
    platform.komorebic("unmanage")
    checks.check(
        f"unmanage: the daemon lets go of {title}, which stays open",
        platform.wait_for(has_let_go, timeout=10),
    )
    area = platform.work_area()

    def left_alone() -> bool:
        window = platform.os_window(target)
        return (
            window is not None and window.frame.inside(area, 2) and not _on_a_tile(platform, target)
        )

    if not checks.check(
        f"{title} is inside the work area and not on a tile of the layout",
        platform.wait_for(left_alone, timeout=10),
    ):
        print(describe(platform, clients))
    platform.komorebic("manage")
    checks.check(f"manage: {title} is tiled again", platform.wait_for(has_it, timeout=10))
