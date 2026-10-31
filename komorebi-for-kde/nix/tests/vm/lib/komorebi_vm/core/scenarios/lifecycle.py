"""The daemon stops, its windows stay, and a daemon that starts again takes them up."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client, Platform


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Stops the daemon and starts it again, with `clients` open, tiled windows of the first
    workspace.

    What the daemon knew about workspaces is not asked for after the start, only that every window
    that was in view is managed and tiled again.
    """
    seen = [c for c in clients if not platform.hidden(c)]
    platform.komorebic("stop")
    checks.check(
        "the daemon stops answering",
        platform.wait_for(lambda: platform.komorebic_status("state")[0] != 0, timeout=20),
    )
    checks.check(
        "every window is still there",
        all(platform.os_window(c) is not None for c in clients),
    )

    platform.start_komorebi()
    checks.check(
        "the daemon manages every window that was in view",
        platform.wait_for(lambda: platform.state().managed() >= len(seen), timeout=30),
    )
    area = platform.work_area()
    frames = [w.frame for c in seen if (w := platform.os_window(c)) is not None]
    checks.check("... they all have a frame", len(frames) == len(seen))
    checks.check("... inside the work area", all(f.inside(area) for f in frames))
    checks.check(
        "... and not on each other",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
    fresh = platform.spawn("after-the-restart")
    checks.check(
        "a window opened now is managed too",
        platform.wait_for(lambda: platform.state().workspace_of(fresh.title) is not None),
    )
