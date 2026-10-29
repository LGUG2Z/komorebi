"""A window sent to another workspace is put away, and switching to it brings it and takes the
others away."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees, os_focused


def _in_view(platform: Platform, clients: list[Client]) -> list[str]:
    return sorted(c.title for c in clients if not platform.hidden(c))


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Sends the focused one of `clients` to workspace 1, switches to it and back.

    `clients` are tiled windows of workspace 0, each in a container of its own, with more than one
    of them. Workspace 1 is empty.
    """
    last = clients[-1]
    rest = sorted(c.title for c in clients if c != last)
    platform.activate(last)
    platform.wait_for(lambda: focus_agrees(platform, clients, last.title))
    checks.check(
        "every window is in view", _in_view(platform, clients) == sorted(c.title for c in clients)
    )

    platform.komorebic("send-to-workspace", "1")
    checks.check(
        f"after 'send-to-workspace 1' {last.title} is put away",
        platform.wait_for(lambda: platform.hidden(last)),
    )
    checks.check(
        "... the daemon has it on workspace 1",
        platform.wait_for(lambda: platform.state().containers(1) == [[last.title]]),
    )
    checks.check(
        "... and the others are still in view",
        platform.wait_for(lambda: _in_view(platform, clients) == rest),
    )

    platform.komorebic("focus-workspace", "1")
    checks.check(
        "after 'focus-workspace 1' only the window sent there is in view",
        platform.wait_for(lambda: _in_view(platform, clients) == [last.title]),
    )
    checks.check(
        "... it has the focus, and the daemon agrees",
        platform.wait_for(lambda: focus_agrees(platform, clients, last.title)),
    )

    platform.komorebic("focus-workspace", "0")
    checks.check(
        "after 'focus-workspace 0' the others are in view and the one sent away is put away",
        platform.wait_for(lambda: _in_view(platform, clients) == rest),
    )
    checks.check(
        "... and one of them has the focus",
        platform.wait_for(lambda: os_focused(platform, clients) in rest),
    )
