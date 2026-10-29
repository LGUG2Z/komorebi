"""A window moved to a workspace takes the focus there, and the last workspace is one command
away."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.support import focus_agrees


def _in_view(platform: Platform, clients: list[Client]) -> list[str]:
    return sorted(c.title for c in clients if not platform.hidden(c))


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Moves the last of `clients`, which are tiled windows of workspace 0 in a container each,
    to workspace 1 with `move-to-workspace`, goes back and forth with `focus-last-workspace`, and
    moves it home. Workspace 1 is empty."""
    target = clients[-1]
    rest = sorted(c.title for c in clients if c != target)
    everyone = sorted(c.title for c in clients)
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))

    platform.komorebic("move-to-workspace", "1")
    checks.check(
        "after 'move-to-workspace 1' the window is there, in view, and the others are put away",
        platform.wait_for(lambda: _in_view(platform, clients) == [target.title], timeout=20),
    )
    checks.check(
        "... the daemon has it on workspace 1",
        platform.wait_for(lambda: platform.state().containers(1) == [[target.title]]),
    )
    checks.check(
        "... and it has the focus",
        platform.wait_for(lambda: focus_agrees(platform, clients, target.title)),
    )

    platform.komorebic("focus-last-workspace")
    checks.check(
        "'focus-last-workspace' brings the others back and puts the window away",
        platform.wait_for(lambda: _in_view(platform, clients) == rest, timeout=20),
    )
    platform.komorebic("focus-last-workspace")
    checks.check(
        "... and again, it goes back to the window",
        platform.wait_for(lambda: _in_view(platform, clients) == [target.title], timeout=20),
    )

    platform.komorebic("move-to-workspace", "0")
    checks.check(
        "'move-to-workspace 0' has every window in view again",
        platform.wait_for(lambda: _in_view(platform, clients) == everyone, timeout=20),
    )
    checks.check(
        "... tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    checks.check(
        "... and workspace 1 is empty",
        platform.wait_for(lambda: platform.state().containers(1) == []),
    )
