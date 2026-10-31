"""The focus moves where komorebic says, and the operating system and the daemon agree on it."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, os_focused, positions


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Moves the focus among `clients`, which are tiled windows of the first workspace, each in a
    container of its own, with `focus` and `cycle-focus`."""
    first = clients[0].title
    before = positions(platform, clients)

    platform.activate(clients[0])
    checks.check(
        f"{first} has the focus, and the daemon agrees",
        platform.wait_for(lambda: focus_agrees(platform, clients, first)),
    )

    platform.komorebic("focus", "right")
    checks.check(
        "after 'focus right' the operating system has given the focus to another window",
        platform.wait_for(lambda: os_focused(platform, clients) not in ("", first)),
    )
    moved_to = os_focused(platform, clients)
    checks.check(
        "... and the daemon's state focuses the same one",
        platform.wait_for(lambda: focus_agrees(platform, clients, moved_to)),
    )

    platform.komorebic("focus", "left")
    checks.check(
        "'focus left' gives the focus back",
        platform.wait_for(lambda: focus_agrees(platform, clients, first)),
    )

    # the next container is whichever follows in the daemon's own order
    order = [titles[0] for titles in platform.state().containers() if titles]
    after = order[(order.index(first) + 1) % len(order)]
    platform.komorebic("cycle-focus", "next")
    checks.check(
        f"'cycle-focus next' focuses {after}, the next container in the daemon's order",
        platform.wait_for(lambda: focus_agrees(platform, clients, after)),
    )
    platform.komorebic("cycle-focus", "previous")
    checks.check(
        "'cycle-focus previous' goes back",
        platform.wait_for(lambda: focus_agrees(platform, clients, first)),
    )
    checks.check("moving the focus moved no window", positions(platform, clients) == before)
