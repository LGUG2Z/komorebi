"""What only KDE can tell about workspaces being KWin's own virtual desktops, per output.

The shared `workspaces` scenario (run by `core-e2e`) checks that `send-to-workspace` and
`focus-workspace` put a window away and bring it back, with the focus agreed. What is here is what
a real KWin adds: the daemon turns on KWin's per-output virtual desktops and puts the user's
setting back when it stops; a window sent to another workspace is on another desktop while the one
in front keeps showing; `focus-workspace` has KWin show the other desktop and focus the window
there, which the daemon agrees with and without parking anything; a switch by anything else (a
pager, a shortcut, an app raising a window of another desktop) is followed once, with nothing
flip-flopping; and a window that somebody moves to another desktop (the window menu's "Move to
Desktop") moves to that workspace.
"""

import time

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform

TITLES = ("w1", "w2", "w3")


def _focus_changes(platform: KdePlatform) -> int:
    return sum(1 for e in daemon.events(platform.kde.komorebi_log()) if e.kind == "FocusChange")


def _ws(platform: KdePlatform) -> int:
    """The workspace the daemon has in front on the (only) monitor."""
    return platform.state().monitors[0].focused_workspace


def _active_title(platform: KdePlatform) -> str:
    active = platform.kde.active()
    return platform.kde.title(active) if active else ""


def _per_output(platform: KdePlatform) -> bool:
    return platform.kde.kwinrc("Windows", "PerOutputVirtualDesktops") == "true"


def run(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Starts the daemon with three windows, and walks through the desktops. The daemon is stopped
    at the end, cleanly. It must not be running when this starts, with no window open."""
    kde = platform.kde
    scale = kde.scale

    checks.check(
        "per-output virtual desktops are off before the daemon starts", not _per_output(platform)
    )
    platform.start_komorebi()
    for title in TITLES:
        platform.spawn(title)
    checks.check("the daemon manages the three", kde.wait_for(lambda: kde.managed(0) == 3, 20))

    checks.check(
        "the daemon turned on per-output virtual desktops in kwinrc", _per_output(platform)
    )
    checks.check(
        "all three windows are on desktop 1, which is showing",
        all(kde.desk(kde.wid(t)) == 1 for t in TITLES) and kde.shown() == 1,
    )

    # one window goes to workspace 1 and stays there
    moved = _active_title(platform)
    mid = kde.wid(moved)
    kde.komorebic("send-to-workspace", "1")
    checks.check(
        f"{moved} is on desktop 2, and desktop 1 is still showing",
        kde.wait_for(lambda: kde.desk(mid) == 2 and kde.shown() == 1 and _ws(platform) == 0),
    )
    checks.check(
        "the other two are on workspace 0, and the daemon has the moved one on workspace 1",
        kde.wait_for(lambda: kde.managed(0) == 2 and kde.managed(1) == 1),
    )
    stayed = [t for t in TITLES if t != moved]

    # komorebic switches workspace: KWin shows the desktop, the daemon hides nothing
    kde.komorebic("focus-workspace", "1")
    checks.check(
        "desktop 2 is showing and the daemon agrees",
        kde.wait_for(lambda: kde.shown() == 2 and _ws(platform) == 1),
    )
    checks.check(
        "KWin focused the window of that desktop and the daemon knows it",
        kde.wait_for(lambda: _active_title(platform) == moved and kde.focused_title(1) == moved),
    )
    checks.check(
        "the windows of desktop 1 never left it", all(kde.desk(kde.wid(t)) == 1 for t in stayed)
    )
    checks.check("the daemon made one desktop, for workspace 1, and no other", kde.desktops() == 2)
    kde.komorebic("focus-workspace", "0")
    checks.check("back on desktop 1", kde.wait_for(lambda: kde.shown() == 1 and _ws(platform) == 0))

    # something else switches: a pager or a shortcut
    kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin setCurrentDesktop i 2 > /dev/null")
    checks.check("the daemon followed to workspace 1", kde.wait_for(lambda: _ws(platform) == 1))
    checks.check(
        "and agrees with KWin on the focused window",
        kde.wait_for(
            lambda: (
                _active_title(platform) != "" and _active_title(platform) == kde.focused_title(1)
            )
        ),
    )

    # an app raises a window of another desktop
    kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin setCurrentDesktop i 1 > /dev/null")
    kde.wait_for(lambda: _ws(platform) == 0)
    time.sleep(1 * scale)
    before = _focus_changes(platform)
    kde.activate(mid)
    checks.check(
        "KWin switched to the window's desktop and the daemon followed",
        kde.wait_for(lambda: kde.shown() == 2 and _ws(platform) == 1),
    )
    checks.check(
        f"KWin and the daemon agree on the focused window: {moved}",
        kde.wait_for(lambda: _active_title(platform) == moved and kde.focused_title(1) == moved),
    )
    time.sleep(3 * scale)
    after = _focus_changes(platform)
    print(f"     focus changes the daemon handled meanwhile: {after - before}")
    checks.check(
        f"nothing is flip-flopping (at most 6 focus events, got {after - before})",
        after - before <= 6,
    )
    time.sleep(3 * scale)
    final = _focus_changes(platform)
    checks.check(f"and it stays quiet afterwards ({final - after} more)", final - after <= 1)

    # somebody moves a window to another desktop (the window menu's Move to Desktop)
    other = kde.wid(stayed[0])
    kde.sh(f"kdotool set_desktop_for_window {other} 2")
    checks.check(
        "the daemon moved it to workspace 1: one window left on workspace 0, two on workspace 1",
        kde.wait_for(lambda: kde.managed(0) == 1 and kde.managed(1) == 2),
    )
    checks.check("and left it where KWin put it", kde.desk(other) == 2)
    checks.check(
        "KWin carried out every command", "the bridge could not do" not in kde.komorebi_log()
    )

    # a clean stop
    kde.komorebic("stop")
    checks.check("the daemon exited", kde.wait_for(lambda: not kde.komorebi_running()))
    checks.check(
        "per-output virtual desktops are back as they were (off)", not _per_output(platform)
    )
    checks.check("the desktops the daemon made stay: they are the user's now", kde.desktops() == 2)
