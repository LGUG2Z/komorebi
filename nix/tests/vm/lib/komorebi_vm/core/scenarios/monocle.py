"""In monocle one window has the work area and the others are put away; toggling gives it back."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, os_focused, positions

# how much smaller than the work area a window in monocle may be: the padding of the layout
PADDING = 120


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Toggles monocle on the focused one of `clients`, which are tiled windows of the first
    workspace, each in a container of its own, and toggles it off again."""
    last = clients[-1]
    platform.activate(last)
    platform.wait_for(lambda: focus_agrees(platform, clients, last.title))
    before = positions(platform, clients)
    area = platform.work_area()

    def fills_the_work_area() -> bool:
        window = platform.os_window(last)
        return (
            window is not None
            and window.frame.w >= area.w - PADDING
            and window.frame.h >= area.h - PADDING
        )

    platform.komorebic("toggle-monocle")
    checks.check(
        "in monocle the focused window fills the work area",
        platform.wait_for(fills_the_work_area, timeout=20),
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
