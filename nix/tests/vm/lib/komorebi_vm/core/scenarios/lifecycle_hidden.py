"""A window that was put away on another workspace is not lost when the daemon restarts."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.support import focus_agrees


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
