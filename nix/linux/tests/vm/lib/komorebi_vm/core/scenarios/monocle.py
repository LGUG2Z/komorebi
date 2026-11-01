"""In monocle one window has the work area and the others are put away; toggling gives it back."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, os_focused, positions

# how much smaller than the work area a window in monocle may be: the padding of the layout
PADDING = 120


def _fills(platform: Platform, client: Client) -> bool:
    """The window has the work area, up to the padding of the layout."""
    window = platform.os_window(client)
    area = platform.work_area()
    return (
        window is not None
        and window.frame.w >= area.w - PADDING
        and window.frame.h >= area.h - PADDING
    )


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Toggles monocle on the focused one of `clients`, which are tiled windows of the first
    workspace, each in a container of its own, and toggles it off again."""
    last = clients[-1]
    platform.activate(last)
    platform.wait_for(lambda: focus_agrees(platform, clients, last.title))
    before = positions(platform, clients)

    def fills_the_work_area() -> bool:
        return _fills(platform, last)

    platform.komorebic("toggle-monocle")
    checks.check(
        "in monocle the focused window fills the work area",
        platform.wait_for(fills_the_work_area, timeout=20),
    )
    window = platform.os_window(last)
    checks.check(
        "... without going past the work area",
        window is not None and window.frame.inside(platform.work_area()),
    )
    checks.check(
        "... the daemon's state has it as the monocle container",
        platform.wait_for(
            lambda: [w.title for w in platform.state().workspace().monocle or []] == [last.title]
        ),
    )
    checks.check(
        "... and every other window is put away",
        platform.wait_for(
            lambda: (
                all(platform.hidden(c) for c in clients if c != last) and not platform.hidden(last)
            )
        ),
    )
    checks.check("... and it still has the focus", os_focused(platform, clients) == last.title)

    platform.komorebic("toggle-monocle")
    checks.check(
        "toggling back puts every window in view again",
        platform.wait_for(lambda: not any(platform.hidden(c) for c in clients), timeout=20),
    )
    checks.check(
        "... each on the tile it had",
        platform.wait_for(lambda: positions(platform, clients) == before, timeout=20),
    )
    checks.check(
        "... and the state has no monocle container",
        platform.wait_for(lambda: platform.state().workspace().monocle is None),
    )


def focus_behaviour(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """What `monocle-focus-behaviour` makes of focusing in a direction in a monocle: with `no-op`
    the window keeps the work area, and with `cycle` another window gets it.

    `clients` are two or more tiled windows of the first workspace, each in a container of its
    own. The monocle is off and the behaviour is `no-op` at the end.
    """
    first = clients[0]
    platform.activate(first)
    platform.wait_for(lambda: focus_agrees(platform, clients, first.title))
    before = positions(platform, clients)

    platform.komorebic("monocle-focus-behaviour", "no-op")
    platform.komorebic("toggle-monocle")
    platform.wait_for(lambda: _fills(platform, first), timeout=20)
    platform.komorebic("focus", "right")
    checks.check(
        "with the no-op behaviour, focusing in a direction leaves the window the work area",
        not platform.wait_for(lambda: os_focused(platform, clients) != first.title, timeout=3)
        and _fills(platform, first),
    )

    platform.komorebic("monocle-focus-behaviour", "cycle")
    platform.komorebic("focus", "right")

    def another_has_it() -> bool:
        title = os_focused(platform, clients)
        return title not in ("", first.title) and _fills(platform, Client(title))

    checks.check(
        "with the cycle behaviour, focusing in a direction gives another window the work area",
        platform.wait_for(another_has_it, timeout=20),
    )
    checks.check(
        "... and the daemon's state has that window as the monocle container",
        platform.wait_for(
            lambda: (
                [w.title for w in platform.state().workspace().monocle or []]
                == [os_focused(platform, clients)]
            )
        ),
    )

    platform.komorebic("toggle-monocle")
    platform.komorebic("monocle-focus-behaviour", "no-op")
    checks.check(
        "the windows are tiled again, each on the tile it had",
        platform.wait_for(lambda: positions(platform, clients) == before, timeout=20),
    )
