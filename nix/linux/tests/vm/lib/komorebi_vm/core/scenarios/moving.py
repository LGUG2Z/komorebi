"""Windows swap tiles on `move` and `cycle-move`, and the focus stays with the window."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, positions


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Moves the first of `clients`, which are tiled windows of the first workspace, each in a
    container of its own, to the tile on its right and back."""
    first = clients[0].title
    platform.activate(clients[0])
    platform.wait_for(lambda: focus_agrees(platform, clients, first))
    before = positions(platform, clients)

    def swapped_with_another() -> bool:
        now = positions(platform, clients)
        return any(
            title != first
            and now[first] == before[title]
            and now[title] == before[first]
            and now[first] != before[first]
            for title in before
        )

    platform.komorebic("move", "right")
    checks.check(
        f"after 'move right' {first} and a neighbor have swapped tiles",
        platform.wait_for(swapped_with_another, timeout=20),
    )
    checks.check(
        f"... and {first} keeps the focus",
        platform.wait_for(lambda: focus_agrees(platform, clients, first)),
    )
    others = {t: p for t, p in positions(platform, clients).items() if t != first}
    checks.check(
        "... and the other windows are on tiles of their own",
        len(set(others.values()) | {positions(platform, clients)[first]}) == len(clients),
    )

    platform.komorebic("cycle-move", "previous")
    checks.check(
        "'cycle-move previous' puts every window back",
        platform.wait_for(lambda: positions(platform, clients) == before, timeout=20),
    )
