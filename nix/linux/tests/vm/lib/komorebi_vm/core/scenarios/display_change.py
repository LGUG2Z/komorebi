"""A display whose mode changes: the daemon's monitor follows, and the windows stay put."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.hotplug import _in_one_of
from komorebi_vm.core.scenarios.support import positions


def _size_follows(platform: Platform, index: int, size: tuple[int, int]) -> bool:
    """The operating system's display and the daemon's monitor both have this size."""
    displays, monitors = platform.displays(), platform.state().monitors
    return (
        index < len(displays)
        and index < len(monitors)
        and (displays[index].w, displays[index].h) == size
        and (monitors[index].size.w, monitors[index].size.h) == size
    )


def _clear(platform: Platform, clients: list[Client]) -> bool:
    """Every window is on a display, and none is over another."""
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    return _in_one_of(platform, clients, platform.displays()) and not any(
        a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Changes the mode of a display to a smaller one and back, and checks that the daemon's
    monitor has the size of each, that the windows are inside the displays and apart while it is
    small, and that they are where they were when the display is as it was.

    `clients` are windows the daemon manages, each in a container of its own. A second display is
    plugged in first if the platform has only one, and the platform says which display can change
    its mode: where none can, the scenario is skipped.
    """
    if len(platform.displays()) < 2:
        platform.add_display()
        platform.wait_for(lambda: len(platform.state().monitors) == len(platform.displays()))
    index = platform.resizable_display()
    if index is None:
        checks.skip(
            "a display changes its mode", "the platform cannot change the mode of a display"
        )
        return
    before = platform.displays()[index]
    size = (1280, 720) if before.w > 1280 else (1024, 768)
    there = positions(platform, clients)

    platform.resize_display(index, *size)
    checks.check(
        f"the display has the mode {size[0]}x{size[1]} and the daemon's monitor follows",
        platform.wait_for(lambda: _size_follows(platform, index, size), timeout=30),
    )
    checks.check(
        "... every window is on a display and clear of the others",
        platform.wait_for(lambda: _clear(platform, clients), timeout=30),
    )

    platform.resize_display(index, before.w, before.h)
    checks.check(
        "changed back, the display and the monitor have their size again",
        platform.wait_for(lambda: _size_follows(platform, index, (before.w, before.h)), timeout=30),
    )
    checks.check(
        "... and every window is where it was",
        platform.wait_for(lambda: positions(platform, clients) == there, timeout=30),
    )
