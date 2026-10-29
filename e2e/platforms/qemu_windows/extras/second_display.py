"""A second display, before any is unplugged: its monitor and a window sent to it."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import WindowsPlatform


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Plugs in a second display, checks the monitor the daemon has for it,
    sends the last of `clients` there and checks where it is and what has
    the focus. The display stays plugged in."""
    platform.add_display()
    checks.check(
        "the daemon has a monitor for the second display",
        platform.wait_for(lambda: len(platform.state().monitors) == 2, timeout=30),
    )
    displays = platform.displays()
    sizes = [(m.size.w, m.size.h) for m in platform.state().monitors]
    checks.check(
        "each monitor has the size of its display",
        sizes == [(d.w, d.h) for d in displays],
    )

    moved = clients[-1]
    platform.activate(moved)
    platform.komorebic("move-to-monitor", "1")
    checks.check(
        "`move-to-monitor 1` takes the window to the second monitor",
        platform.wait_for(lambda: platform.state().workspace_of(moved.title, monitor=1) == 0),
    )
    checks.check(
        "... where the operating system has it inside the second display",
        platform.wait_for(
            lambda: (w := platform.os_window(moved)) is not None
            and w.frame.inside(displays[1], 1)
        ),
    )
    checks.check(
        "... and the others stay on the first monitor",
        all(platform.state().workspace_of(c.title, monitor=0) is not None
            for c in clients[:-1]),
    )
    checks.check(
        "the daemon's focus is the operating system's: the moved window",
        platform.wait_for(
            lambda: platform.state().focused_title(monitor=1) == moved.title
            and (w := platform.os_window(moved)) is not None and w.focused
        ),
    )
