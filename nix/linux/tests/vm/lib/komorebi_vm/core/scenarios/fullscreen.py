"""A window the user takes fullscreen is left alone by the daemon, and goes back to its tile."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees


def _covers(frame: Rect, display: Rect) -> bool:
    """The frame has the whole display, give or take a pixel."""
    return (
        frame.x <= display.x + 1
        and frame.y <= display.y + 1
        and frame.x + frame.w >= display.x + display.w - 1
        and frame.y + frame.h >= display.y + display.h - 1
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Takes the last of `clients` fullscreen, opens a window, and brings it back.

    `clients` are tiled windows of the first workspace on the first display, each in a container
    of its own. The window opened is closed at the end.
    """
    target = clients[-1]
    count = platform.state().managed()
    display = platform.displays()[0]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))
    # the daemon asks again for the frames of what it moved, for a little while after a command
    platform.wait_for(lambda: False, timeout=3)

    def covers() -> bool:
        window = platform.os_window(target)
        return window is not None and _covers(window.frame, display)

    platform.fullscreen(target)
    checks.check(
        "a window taken fullscreen covers the display",
        platform.wait_for(covers, timeout=20),
    )
    extra = platform.spawn("fullscreen-extra")
    checks.check(
        "the daemon manages a window that opens meanwhile",
        platform.wait_for(lambda: platform.state().managed() == count + 1, timeout=20),
    )
    platform.wait_for(lambda: False, timeout=3)
    checks.check("... and it does not take the window out of fullscreen", covers())

    platform.fullscreen(target, on=False)

    def in_a_tile() -> bool:
        window = platform.os_window(target)
        return (
            window is not None
            and window.frame.w < display.w - 100
            and window.frame.inside(display, 1)
        )

    checks.check(
        "back from fullscreen it is in a tile, inside the display",
        platform.wait_for(in_a_tile, timeout=20),
    )
    frames = [w.frame for c in [*clients, extra] if (w := platform.os_window(c)) is not None]
    checks.check(
        "... not on any other window",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
    checks.check(
        "... and the daemon still has every window", platform.state().managed() == count + 1
    )
    platform.close(extra)
    platform.wait_for(lambda: platform.state().managed() == count, timeout=20)
