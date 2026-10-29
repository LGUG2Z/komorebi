"""The mode of a display changes while windows are on it.

A draft of what could be a shared scenario: a platform that can change the
size of a display says so, and the checks need nothing of Windows.
"""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import WindowsPlatform

SIZES = ((1280, 720), (800, 600))


def _frames(platform: WindowsPlatform, clients: list[Client]):
    return [w.frame for c in clients if (w := platform.os_window(c)) is not None]


def _fits(platform: WindowsPlatform, clients: list[Client]) -> bool:
    """Every window is inside the work area of the second monitor, none on another."""
    monitors = platform.state().monitors
    if len(monitors) < 2:
        return False
    frames = _frames(platform, clients)
    return (
        len(frames) == len(clients)
        and all(f.inside(monitors[1].work_area, 1) for f in frames)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1:])
    )


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Plugs in a second display, sends all but the first of `clients` to it,
    changes its mode to two others and back, and checks what the daemon makes
    of each. The display stays plugged in, at its first mode."""
    platform.add_display()
    checks.check(
        "the daemon has a monitor for the second display",
        platform.wait_for(lambda: len(platform.state().monitors) == 2, timeout=30),
    )
    moved = clients[1:]
    for client in moved:
        platform.activate(client)
        platform.komorebic("move-to-monitor", "1")
        platform.wait_for(
            lambda c=client: platform.state().workspace_of(c.title, monitor=1) is not None
        )
    checks.check("the windows are tiled on the second monitor", _fits(platform, moved))
    first = platform.displays()[1]
    before = _frames(platform, moved)

    for width, height in SIZES:
        platform.resize_display(1, width, height)
        size = f"{width}x{height}"
        checks.check(
            f"after the display is changed to {size} the daemon's monitor has that size",
            platform.wait_for(
                lambda: (m := platform.state().monitors[1]).size.w == width
                and m.size.h == height,
                timeout=30,
            ),
        )
        checks.check(
            f"... and the windows are inside its work area, not on each other, at {size}",
            platform.wait_for(lambda: _fits(platform, moved), timeout=30),
        )

    platform.resize_display(1, first.w, first.h)
    tolerance = platform.tolerance
    checks.check(
        "changed back, the windows are where they were",
        platform.wait_for(
            lambda: len(after := _frames(platform, moved)) == len(before)
            and all(
                abs(a.x - b.x) <= tolerance and abs(a.y - b.y) <= tolerance
                and abs(a.w - b.w) <= tolerance and abs(a.h - b.h) <= tolerance
                for a, b in zip(after, before, strict=True)
            ),
            timeout=30,
        ),
    )
