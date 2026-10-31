"""A window that was put away on another workspace is not lost when the daemon restarts."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import describe, focus_agrees


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Sends the last of `clients`, which are tiled windows of workspace 0, to workspace 1,
    stops the daemon and starts it again.

    Where the daemon puts the window afterwards is not asked for, only that it manages every
    window again and that the ones in view are inside the work area and clear of each other.
    """
    target = clients[-1]
    platform.activate(target)
    platform.wait_for(lambda: focus_agrees(platform, clients, target.title))
    platform.komorebic("send-to-workspace", "1")
    checks.check(
        f"{target.title} is put away on workspace 1",
        platform.wait_for(lambda: platform.state().containers(1) == [[target.title]], timeout=20),
    )

    platform.komorebic("stop")
    checks.check(
        "the daemon stops answering",
        platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20),
    )
    platform.start_komorebi()
    checks.check(
        "the daemon manages every window again, the one that was put away too",
        platform.wait_for(lambda: platform.state().managed() >= len(clients), timeout=30),
    )
    checks.check(
        f"... and it has {target.title} on a workspace",
        platform.wait_for(lambda: platform.state().workspace_of(target.title) is not None),
    )
    area = platform.work_area()
    frames = [
        w.frame
        for c in clients
        if not platform.hidden(c) and (w := platform.os_window(c)) is not None
    ]
    checks.check(
        "... the windows in view are inside the work area", all(f.inside(area) for f in frames)
    )
    checks.check(
        "... and not on each other",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )


def crash(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Turns a monocle on, which puts the other windows away, kills the daemon, and starts it
    again: nothing is left out of sight for the new daemon to find, and it manages every window.

    `clients` are two windows or more, each in a container of its own. They are gathered on the
    first workspace, which is shown, first: a window another scenario left on a workspace that is
    not in view would be out of sight after the restart on a platform that keeps it there. The
    monocle is off at the end.
    """
    target, other = clients[-1], clients[0]

    def gather() -> None:
        platform.komorebic("focus-workspace", "0")
        for client in clients:
            home = platform.state().workspace_of(client.title)
            if home not in (0, None):
                platform.komorebic("focus-workspace", str(home))
                platform.activate(client)
                platform.komorebic("move-to-workspace", "0")
        platform.komorebic("focus-workspace", "0")
        platform.wait_for(
            lambda: all(platform.state().workspace_of(c.title) == 0 for c in clients), timeout=20
        )
        platform.wait_for(lambda: not any(platform.hidden(c) for c in clients), timeout=20)

    gather()

    def in_front() -> None:
        # a window that already has the focus gives the daemon no event: another one first
        platform.activate(other)
        platform.activate(target)
        platform.wait_for(lambda: focus_agrees(platform, clients, target.title))

    def only_target_in_view() -> bool:
        return all(platform.hidden(c) for c in clients if c != target) and not platform.hidden(
            target
        )

    in_front()
    platform.komorebic("toggle-monocle")
    checks.check(
        "with a monocle on, the other windows are out of sight",
        platform.wait_for(only_target_in_view, timeout=20),
    )
    platform.crash()
    checks.check(
        "the killed daemon no longer answers",
        platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20),
    )

    platform.start_komorebi()
    checks.check(
        "the next daemon manages every window",
        platform.wait_for(lambda: platform.state().managed() >= len(clients), timeout=30),
    )
    if not checks.check(
        "... and none is left out of sight",
        platform.wait_for(lambda: not any(platform.hidden(c) for c in clients), timeout=20),
    ):
        print(describe(platform, clients))
    area = platform.work_area()
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    checks.check(
        "... inside the work area and not on each other",
        len(frames) == len(clients)
        and all(f.inside(area) for f in frames)
        and not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )

    in_front()
    platform.komorebic("toggle-monocle")
    checks.check(
        "after a crash and a restart a monocle still puts the other windows away",
        platform.wait_for(only_target_in_view, timeout=20),
    )
    platform.komorebic("toggle-monocle")
    if not checks.check(
        "... and turned off, nothing is out of sight",
        platform.wait_for(lambda: not any(platform.hidden(c) for c in clients), timeout=20),
    ):
        print(describe(platform, clients))
