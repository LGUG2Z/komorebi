"""Each layout the daemon has tiles the windows it is given: where it says, none over another."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.tiling import _where_the_layout_says

# the layouts both ports have, but for scrolling, which does not fit the work area
LAYOUTS = (
    "bsp",
    "columns",
    "rows",
    "vertical-stack",
    "horizontal-stack",
    "ultrawide-vertical-stack",
    "grid",
    "right-main-vertical-stack",
)


def tiled(platform: Platform, clients: list[Client]) -> bool:
    """Every window is where the daemon's own layout puts it, as the OS reports it."""
    workspace = platform.state().workspace()
    layout = workspace.latest_layout
    frames = [platform.os_window(c) for c in clients]
    if len(layout) != len(clients) or any(f is None for f in frames):
        return False
    by_title = {w.title: layout[i] for i, c in enumerate(workspace.containers) for w in c[:1]}
    return all(
        f is not None
        and f.title in by_title
        and _where_the_layout_says(f.frame, by_title[f.title], platform.tolerance)
        for f in frames
    )


def _check_layout(platform: Platform, checks: Checks, clients: list[Client], layout: str) -> None:
    label = f"{layout}, {len(clients)} windows"
    platform.komorebic("change-layout", layout)
    checks.check(
        f"{label}: every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    area = platform.work_area()
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    checks.check(f"{label}: ... inside the work area", all(f.inside(area) for f in frames))
    checks.check(
        f"{label}: ... and not on each other",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Changes the layout of the first workspace to each of `LAYOUTS` with `clients`, which are
    tiled windows in a container each, and then with two windows more, which it closes.

    The layout is `bsp` again at the end.
    """
    for layout in LAYOUTS:
        _check_layout(platform, checks, clients, layout)

    more = [platform.spawn("layouts-extra1"), platform.spawn("layouts-extra2")]
    everyone = clients + more
    checks.check(
        "two windows more are managed",
        platform.wait_for(lambda: platform.state().managed() == len(everyone), timeout=20),
    )
    for layout in LAYOUTS:
        _check_layout(platform, checks, everyone, layout)

    platform.komorebic("change-layout", "bsp")
    for client in more:
        platform.close(client)
    checks.check(
        "the two windows are closed and the others tiled again",
        platform.wait_for(
            lambda: platform.state().managed() == len(clients) and tiled(platform, clients),
            timeout=20,
        ),
    )
