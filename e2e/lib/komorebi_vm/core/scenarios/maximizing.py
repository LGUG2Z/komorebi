"""A maximized window has the work area, with the others left where they are behind it."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees, positions


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Toggles maximize on the last of `clients`, which are tiled windows of the first workspace,
    each in a container of its own, and toggles it off again."""
    target = clients[-1]
    others = [c for c in clients if c != target]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))
    area = platform.work_area()
    behind = positions(platform, others)

    def fills() -> bool:
        window = platform.os_window(target)
        return (
            window is not None
            and window.frame.w >= area.w - platform.tolerance
            and window.frame.h >= area.h - platform.tolerance
        )

    def maximized() -> str | None:
        window = platform.state().workspace().maximized
        return window.title if window else None

    platform.komorebic("toggle-maximize")
    checks.check(
        f"after 'toggle-maximize' {target.title} fills the work area",
        platform.wait_for(fills, timeout=20),
    )
    window = platform.os_window(target)
    checks.check(
        "... without going past the work area",
        window is not None and window.frame.inside(area),
    )
    checks.check(
        "... and the daemon has it as the maximized window",
        platform.wait_for(lambda: maximized() == target.title, timeout=20),
    )
    checks.check(
        "... the others stay where they were",
        positions(platform, others) == behind,
    )

    platform.komorebic("toggle-maximize")
    checks.check(
        "toggled again, the daemon has no maximized window",
        platform.wait_for(lambda: maximized() is None, timeout=20),
    )
    checks.check(
        "... and every window is tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )


# What is asked of the layout, which a maximized window is not in: none of it does anything. Not in
# the list, because komorebi for Windows does not ignore them either: `toggle-float`, which floats
# the maximized window (`user` ends with it), and `stack left` and `stack right`, which stack the
# tiles that are left.
IGNORED = (
    "move left",
    "move right",
    "move up",
    "move down",
    "unstack",
    "cycle-move next",
    "promote",
    "toggle-monocle",
    "resize-edge left increase",
    "send-to-workspace 1",
    "move-to-workspace 1",
    "toggle-lock",
)


def user(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """A window the user maximizes, not the command, is taken out of the layout like one that
    `toggle-maximize` maximized: the daemon has it as its maximized window, the others are left
    alone, what is asked of the layout does nothing, and it goes back to its tile when it is
    restored, by `toggle-maximize` or by the user.

    `clients` are three tiled windows or more of the first workspace, each in a container of its
    own. The first workspace has the second one, so that `send-to-workspace 1` can name it.
    """
    count = len(clients)
    target, other = clients[1], clients[-1]
    area = platform.work_area()
    tiles = positions(platform, clients)

    def maximized() -> str | None:
        window = platform.state().workspace().maximized
        return window.title if window else None

    def containers() -> int:
        return len(platform.state().workspace().containers)

    def fills(client: Client) -> bool:
        window = platform.os_window(client)
        return (
            window is not None
            and window.frame.w >= area.w - 2 * platform.tolerance
            and window.frame.h >= area.h - 4 * platform.tolerance
        )

    platform.user_maximize(target)
    checks.check(
        f"after the user maximizes {target.title} the daemon has it as its maximized window",
        platform.wait_for(lambda: maximized() == target.title, timeout=20),
    )
    checks.check("... it fills the work area", platform.wait_for(lambda: fills(target), timeout=10))
    checks.check(
        "... its tile is gone, and the daemon still has every window",
        containers() == count - 1 and platform.state().managed() == count,
    )

    beside = positions(platform, [c for c in clients if c != target])
    for command in IGNORED:
        platform.komorebic_status(*command.split())
    platform.wait_for(lambda: False, timeout=1)
    checks.check(
        "what is asked of the layout does nothing: it is still maximized, and the others did not"
        " move",
        fills(target)
        and maximized() == target.title
        and positions(platform, [c for c in clients if c != target]) == beside,
    )
    workspace = platform.state().workspace()
    checks.check(
        "... and nothing floats or is in the monocle",
        containers() == count - 1 and workspace.floating == [] and workspace.monocle is None,
    )

    platform.komorebic("toggle-maximize")
    checks.check(
        f"toggle-maximize restores it: nothing is maximized and {target.title} is in its tile",
        platform.wait_for(
            lambda: (
                maximized() is None
                and containers() == count
                and positions(platform, [target]) == {target.title: tiles[target.title]}
            ),
            timeout=20,
        ),
    )

    platform.user_maximize(other)
    checks.check(
        f"the user maximizes {other.title} too",
        platform.wait_for(lambda: maximized() == other.title and containers() == count - 1),
    )
    platform.user_maximize(other, on=False)
    checks.check(
        f"restored by the user, {other.title} is back in its tile and nothing is maximized",
        platform.wait_for(
            lambda: (
                maximized() is None
                and containers() == count
                and positions(platform, [other]) == {other.title: tiles[other.title]}
            ),
            timeout=20,
        ),
    )

    # `toggle-float` is asked of the window in front, and that is the maximized one: it floats and
    # is not maximized any more, as on komorebi for Windows
    platform.user_maximize(target)
    platform.wait_for(lambda: maximized() == target.title, timeout=20)
    platform.komorebic("toggle-float")
    checks.check(
        f"toggle-float floats {target.title}, which the user maximized, and ends the maximize",
        platform.wait_for(
            lambda: (
                maximized() is None
                and [w.title for w in platform.state().workspace().floating] == [target.title]
                and containers() == count - 1
            ),
            timeout=20,
        ),
    )
    platform.komorebic("toggle-float")
    checks.check(
        f"... and again puts {target.title} back in the layout",
        platform.wait_for(
            lambda: (
                containers() == count
                and platform.state().workspace().floating == []
                and tiled(platform, clients)
            ),
            timeout=20,
        ),
    )
