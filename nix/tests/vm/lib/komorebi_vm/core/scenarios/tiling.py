"""Windows that open are tiled: each where the layout says, none over another, all on screen."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform


def _where_the_layout_says(frame: Rect, want: Rect, tolerance: int) -> bool:
    """The same place, and the size up to `tolerance` short of what was asked for."""
    return (
        (frame.x, frame.y) == (want.x, want.y)
        and 0 <= want.w - frame.w <= tolerance
        and 0 <= want.h - frame.h <= tolerance
    )


def run(
    platform: Platform, checks: Checks, titles: tuple[str, ...] = ("win1", "win2", "win3")
) -> list[Client]:
    """Opens a window for each title and checks, against the OS's own account of the frames, that
    the daemon has tiled them.

    The daemon must be running, on a workspace with no window, with a tiling layout. Returns the
    windows it opened, for a scenario that goes on from them.
    """
    clients = [platform.spawn(title) for title in titles]
    area = platform.work_area()

    def managed() -> bool:
        return platform.state().managed() == len(titles)

    checks.check("the daemon manages every window", platform.wait_for(managed, timeout=20))

    def placed() -> bool:
        workspace = platform.state().workspace()
        layout = workspace.latest_layout
        frames = [platform.os_window(c) for c in clients]
        if len(layout) != len(titles) or any(f is None for f in frames):
            return False
        # the layout's rectangles are in the order of the containers, which is not the order the
        # windows opened in: each window is matched with its own container's rectangle
        by_title = {w.title: layout[i] for i, c in enumerate(workspace.containers) for w in c[:1]}
        return all(
            f is not None and _where_the_layout_says(f.frame, by_title[f.title], platform.tolerance)
            for f in frames
        )

    checks.check(
        "every window is where the layout says, as the OS reports it",
        platform.wait_for(placed, timeout=20),
    )

    frames = [f.frame for c in clients if (f := platform.os_window(c)) is not None]
    checks.check("the OS has a frame for every window", len(frames) == len(titles))
    for client, frame in zip(clients, frames, strict=False):
        checks.check(f"{client.title} is inside the work area", frame.inside(area))
    for i, a in enumerate(frames):
        for j in range(i + 1, len(frames)):
            checks.check(
                f"{clients[i].title} and {clients[j].title} do not overlap",
                not a.overlaps(frames[j]),
            )
    checks.check(
        "the windows share the work area: none takes all of its width",
        all(f.w < area.w - platform.tolerance for f in frames),
    )
    return clients
