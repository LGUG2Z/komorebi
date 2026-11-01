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
    checks.check(
        "after a display is unplugged the daemon has one monitor fewer",
        platform.wait_for(lambda: _monitors(platform) == len(platform.displays()), timeout=30),
    )
    checks.check(
        "... every window is on the displays that are left, tiled and clear of each other",
        platform.wait_for(
            lambda: (
                _in_one_of(platform, clients, platform.displays())
                and (
                    len(platform.displays()) > 1
                    or _tiled_in(platform, clients, platform.work_area())
                )
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


def _display_of(frame: Rect, displays: list[Rect]) -> int | None:
    """The display the middle of the frame is on."""
    x, y = frame.x + frame.w // 2, frame.y + frame.h // 2
    for i, d in enumerate(displays):
        if d.x <= x < d.x + d.w and d.y <= y < d.y + d.h:
            return i
    return None


def _split(platform: Platform, clients: list[Client]) -> dict[int, list[Client]]:
    """The windows on each display, by the middle of their frames."""
    displays = platform.displays()
    found: dict[int, list[Client]] = {i: [] for i in range(len(displays))}
    for client in clients:
        window = platform.os_window(client)
        if window is not None and (i := _display_of(window.frame, displays)) is not None:
            found[i].append(client)
    return found


def _containers(platform: Platform, monitor: int) -> int:
    return len(platform.state().workspace(monitor=monitor).containers)


def _each_tiled_on_its_display(platform: Platform, clients: list[Client]) -> bool:
    displays = platform.displays()
    return all(_tiled_in(platform, on, displays[i]) for i, on in _split(platform, clients).items())


def user_move(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """A window that something other than the daemon moves to the other display is tiled on that
    monitor, and the windows that are left are tiled on the first.

    `clients` are windows the daemon manages, each in a container of its own, on the first display,
    two of them or more. A second display is plugged in first if the platform has only one. The
    window that moves is the last of `clients` on the first display, and it stays on the second.
    """
    if len(platform.displays()) < 2:
        platform.add_display()
    platform.wait_for(lambda: _monitors(platform) == len(platform.displays()), timeout=30)
    displays = platform.displays()
    on_first = _split(platform, clients)[0]
    if len(on_first) < 2:
        checks.skip("a window moved to the other display", "fewer than two windows on the first")
        return
    window = on_first[-1]
    first, second = _containers(platform, 0), _containers(platform, 1)
    # the daemon asks again for the frames of the windows it moved, for a little while after a
    # command: a user moves a window later than that
    platform.wait_for(lambda: False, timeout=2)
    platform.move_window(window, displays[1].x + 180, displays[1].y + 300)
    checks.check(
        "the daemon has the window on the other monitor",
        platform.wait_for(
            lambda: (
                _containers(platform, 0) == first - 1 and _containers(platform, 1) == second + 1
            ),
            timeout=30,
        ),
    )
    checks.check(
        "every window is tiled inside the display it is on, and the moved one is on the second",
        platform.wait_for(
            lambda: (
                window in _split(platform, clients)[1]
                and _each_tiled_on_its_display(platform, clients)
            ),
            timeout=30,
        ),
    )
