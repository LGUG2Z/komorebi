"""What only KDE can tell about workspace rules, beside the shared `rules` and `startup_rules`.

A workspace is a virtual desktop of KWin, so a window that a rule sends to another workspace is on
that workspace's desktop, and out of sight on the parking desktop while the desktop in front is
showing. The shared scenarios check where the daemon puts the window and that it is put away; these
check it on KWin's desktops, also for the rules by the name of a workspace.

The suite runs the shared scenarios, then `desktops`, `named_rule` and `cleared`.
"""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform
from komorebi_vm.platforms.kde.support import parked, where_asked


def _on(platform: KdePlatform, title: str) -> int | None:
    return platform.state().workspace_of(title)


def _shown_workspace(platform: KdePlatform) -> int:
    return platform.state().monitors[0].focused_workspace


def desktops(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """An initial rule sends a window to workspace 1: it is on the parking desktop while desktop 1
    shows, on desktop 2, the desktop of its workspace, once that is shown, and tiled where the
    daemon asked.

    The daemon must be running on the first workspace, with no window. It makes workspaces 0 to 2
    (`web`, `code`, `chat`) and leaves the rule, which `cleared` takes away.
    """
    kde = platform.kde
    kde.komorebic("ensure-named-workspaces", "0", "web", "code", "chat")
    kde.komorebic("initial-workspace-rule", "title", "sent-once", "0", "1")
    kde.komorebic("named-workspace-rule", "title", "sent-always", "chat")

    stay = platform.spawn("stays here")
    once = platform.spawn("sent-once one")
    kde.wait_for(lambda: _on(platform, once.title) == 1, timeout=30)
    checks.check(
        "the window the rule names is on the parking desktop while desktop 1 is showing",
        kde.wait_for(
            lambda: (
                parked(platform, once.title)
                and kde.shown() == 1
                and _shown_workspace(platform) == 0
            )
        ),
    )
    checks.check(
        "the other window is on KWin's desktop 1, where it opened",
        _on(platform, stay.title) == 0 and kde.desk(kde.wid(stay.title)) == 1,
    )
    kde.komorebic("focus-workspace", "1")
    checks.check("workspace 1 is shown as KWin's desktop 2", kde.wait_for(lambda: kde.shown() == 2))
    checks.check(
        "and the window is on that desktop, not parked",
        kde.wait_for(
            lambda: kde.desk(kde.wid(once.title)) == 2 and not parked(platform, once.title)
        ),
    )
    checks.check(
        "the window is tiled where the daemon asked",
        kde.wait_for(lambda: where_asked(platform, once.title)),
    )
    kde.komorebic("move-to-workspace", "0")
    kde.wait_for(lambda: _on(platform, once.title) == 0 and kde.desk(kde.wid(once.title)) == 1)
    kde.komorebic("focus-workspace", "0")


def named_rule(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """A rule by the name of a workspace (`chat`) sends a window to it, which is out of sight, and
    on desktop 3 once shown. The shared `rules` checks the daemon's side, and that the rule is
    applied again at the next event. Needs `desktops` to have run."""
    kde = platform.kde
    always = platform.spawn("sent-always one")
    checks.check(
        "a window the rule by the name of a workspace names goes to chat, out of sight",
        kde.wait_for(lambda: _on(platform, always.title) == 2 and parked(platform, always.title)),
    )
    kde.komorebic("focus-workspace", "2")
    checks.check(
        "showing chat brings the window to KWin's desktop 3",
        kde.wait_for(
            lambda: kde.desk(kde.wid(always.title)) == 3 and not parked(platform, always.title)
        ),
    )
    kde.komorebic("move-to-workspace", "0")
    kde.wait_for(lambda: _on(platform, always.title) == 0 and kde.desk(kde.wid(always.title)) == 1)
    kde.komorebic("focus-workspace", "0")


OPENED = ("stays here", "sent-once one", "sent-always one", "sent-once two")


def cleared(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Once every rule is cleared a window that opens stays on the desktop in front. The shared
    `rules` checks the workspace in view, and that the daemon's global state has no rule. The
    windows of these extras are closed after, and the workspace in front is the first, so that the
    shared scenarios find the daemon as they expect."""
    kde = platform.kde
    kde.komorebic("clear-all-workspace-rules")
    later = platform.spawn("sent-once two")
    checks.check(
        "a window that opens now is on the desktop that is in front",
        kde.wait_for(lambda: _on(platform, later.title) == _shown_workspace(platform), timeout=30)
        and kde.desk(kde.wid(later.title)) == kde.shown(),
    )
    for title in OPENED:
        if kde.wid(title) != "":
            platform.close(Client(title))
    kde.wait_for(lambda: kde.managed_all() == 0, timeout=20)
    kde.komorebic("focus-workspace", "0")
