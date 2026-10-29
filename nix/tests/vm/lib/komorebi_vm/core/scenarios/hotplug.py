"""A display that is unplugged takes its windows to the other, and one plugged in is a monitor."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform


def _monitors(platform: Platform) -> int:
    return len(platform.state().monitors)


def _in_one_of(platform: Platform, clients: list[Client], displays: list[Rect]) -> bool:
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    return len(frames) == len(clients) and all(
        any(f.inside(d, 1) for d in displays) for f in frames
    )


def _tiled_in(platform: Platform, clients: list[Client], area: Rect) -> bool:
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    return (
        len(frames) == len(clients)
        and all(f.inside(area, 1) for f in frames)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :])
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Unplugs a display, plugs one in again, and sends a window to it.

    `clients` are windows the daemon manages, each in a container of its own, on any display. The
    platform has one display or more; one is plugged in first if it has only one.
    """
    if len(platform.displays()) < 2:
        platform.add_display()
    checks.check(
        "the daemon has a monitor for each display",
        platform.wait_for(lambda: _monitors(platform) == len(platform.displays()), timeout=30),
    )
    managed = platform.state().managed()

    platform.remove_display()
    first = platform.displays()[0]
    checks.check(
        "after a display is unplugged the daemon has one monitor fewer",
        platform.wait_for(lambda: _monitors(platform) == len(platform.displays()), timeout=30),
    )
    checks.check(
        "... every window is on the displays that are left, tiled and clear of each other",
        platform.wait_for(
            lambda: (
                _in_one_of(platform, clients, platform.displays())
                and (len(platform.displays()) > 1 or _tiled_in(platform, clients, first))
            ),
            timeout=30,
        ),
    )
    checks.check(
        "... and the daemon still manages every window",
        platform.wait_for(lambda: platform.state().managed() == managed, timeout=20),
    )

    platform.add_display()
    checks.check(
        "after a display is plugged in the daemon has a monitor for it again",
        platform.wait_for(lambda: _monitors(platform) == len(platform.displays()), timeout=30),
    )
    checks.check(
        "... still manages every window, each where it can be seen",
        platform.wait_for(
            lambda: (
                platform.state().managed() == managed
                and _in_one_of(platform, clients, platform.displays())
            ),
            timeout=30,
        ),
    )

    # the focused window goes to the other monitor
    displays = platform.displays()
    focused = next(
        (c for c in clients if (w := platform.os_window(c)) is not None and w.focused), None
    )
    checks.check("a window has the focus", focused is not None)
    if focused is None:
        return
    window = platform.os_window(focused)
    assert window is not None
    here = next((i for i, d in enumerate(displays) if window.frame.inside(d, 1)), None)
    checks.check("it is on one of the displays", here is not None)
    if here is None:
        return
    there = 1 if here == 0 else 0
    platform.komorebic("move-to-monitor", str(there))
    checks.check(
        "after move-to-monitor it is on the other display",
        platform.wait_for(
            lambda: (
                (w := platform.os_window(focused)) is not None
                and w.frame.inside(displays[there], 1)
            ),
            timeout=30,
        ),
    )
