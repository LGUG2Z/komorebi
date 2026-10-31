"""A stack shows one window and puts the others away, and the window in view has the focus."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, os_focused


def _sizes(platform: Platform) -> list[int]:
    return sorted(len(titles) for titles in platform.state().containers())


def _away(platform: Platform, clients: list[Client]) -> list[str]:
    return [c.title for c in clients if platform.hidden(c)]


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Stacks the focused one of `clients` onto its neighbor, cycles the stack and takes it apart.

    `clients` are three tiled windows of the first workspace, each in a container of its own,
    and the last of them has the focus.
    """
    last = clients[-1]
    platform.activate(last)
    platform.wait_for(lambda: focus_agrees(platform, clients, last.title))
    checks.check("there are three tiles", _sizes(platform) == [1, 1, 1])
    checks.check("nothing is put away yet", _away(platform, clients) == [])

    platform.komorebic("stack", "left")
    checks.check(
        "two containers, one of them a stack of two",
        platform.wait_for(lambda: _sizes(platform) == [1, 2]),
    )
    checks.check(
        "exactly one window is put away",
        platform.wait_for(lambda: len(_away(platform, clients)) == 1),
    )
    shown = os_focused(platform, clients)
    checks.check(
        "the window in view has the focus", shown != "" and shown not in _away(platform, clients)
    )
    checks.check(
        "... and the daemon's state agrees",
        platform.wait_for(lambda: focus_agrees(platform, clients, shown)),
    )
    frame = platform.os_window(Client(shown))

    platform.komorebic("cycle-stack", "next")
    checks.check(
        "after 'cycle-stack next' the other window of the stack is in view and the first is away",
        platform.wait_for(
            lambda: (
                os_focused(platform, clients) not in ("", shown)
                and _away(platform, clients) == [shown]
            )
        ),
    )
    other = os_focused(platform, clients)
    checks.check(
        "... with the focus agreed",
        platform.wait_for(lambda: focus_agrees(platform, clients, other)),
    )
    now = platform.os_window(Client(other))
    checks.check(
        "... and it fills the tile the first one had",
        frame is not None and now is not None and now.frame == frame.frame,
    )
    platform.komorebic("cycle-stack", "previous")
    checks.check(
        "cycling back, the first is in view again",
        platform.wait_for(
            lambda: os_focused(platform, clients) == shown and len(_away(platform, clients)) == 1
        ),
    )

    platform.komorebic("unstack")
    checks.check("three tiles again", platform.wait_for(lambda: _sizes(platform) == [1, 1, 1]))
    checks.check("nothing is put away", platform.wait_for(lambda: _away(platform, clients) == []))
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    checks.check(
        "and they tile, not on each other",
        len(frames) == len(clients)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
