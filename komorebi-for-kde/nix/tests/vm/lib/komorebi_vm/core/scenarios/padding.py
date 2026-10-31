"""The padding of a workspace and of its containers moves the windows in from the edges and from
each other."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled


def run(
    platform: Platform,
    checks: Checks,
    clients: list[Client],
    configured: tuple[int, int] = (0, 0),
) -> None:
    """Raises the workspace padding and then the container padding of the first workspace, which
    has `clients` tiled in a container each, and sets them back to what the daemon says they were
    (`configured` is the workspace and container padding for a daemon that does not say)."""
    state = platform.state().workspace()
    workspace = configured[0] if state.workspace_padding is None else state.workspace_padding
    container = configured[1] if state.container_padding is None else state.container_padding
    print(f"     padding of the workspace {workspace}, of its containers {container}")
    area = platform.work_area()
    # how much room the windows must lose or regain, in square pixels: a small part of the work
    # area, for the display is not the same size on every platform
    unit = area.w * area.h // 400

    def frames() -> list[Rect]:
        return [w.frame for c in clients if (w := platform.os_window(c)) is not None]

    def margin() -> int:
        """How far the windows are from the left edge of the work area: the padding of the
        workspace and that of the first container."""
        return min(f.x for f in frames()) - area.x

    def room() -> int:
        return sum(f.w * f.h for f in frames())

    def near(value: int, want: int) -> bool:
        return abs(value - want) <= 10

    # what the windows are measured against is what these commands give, not what the config said
    platform.komorebic("focused-workspace-padding", str(workspace))
    platform.komorebic("focused-workspace-container-padding", str(container))
    checks.check(
        "the windows are inside the work area, tiled where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )
    start = margin()
    print(f"     margin {start} with the padding as it is")
    base = room()

    platform.komorebic("focused-workspace-padding", str(workspace + 40))
    checks.check(
        "a workspace padding 40 px more moves the windows 40 px in from the edge",
        platform.wait_for(lambda: near(margin(), start + 40), timeout=20),
    )
    checks.check(
        "... and they stay where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )

    platform.komorebic("focused-workspace-padding", str(workspace))
    platform.komorebic("focused-workspace-container-padding", str(container + 30))
    checks.check(
        "a container padding 30 px more leaves the windows less room",
        platform.wait_for(lambda: room() < base - unit, timeout=20),
    )
    checks.check(
        "... and none is over another, or outside the work area",
        all(f.inside(area) for f in frames())
        and not any(a.overlaps(b) for i, a in enumerate(frames()) for b in frames()[i + 1 :]),
    )

    platform.komorebic("focused-workspace-container-padding", str(container))
    checks.check(
        "with the padding as it was, the windows are as far from the edge as they were",
        platform.wait_for(lambda: near(margin(), start), timeout=20),
    )
    print(f"     margin {margin()} (was {start}), room {room()} (was {base})")
    checks.check(
        "... and have the room they had",
        platform.wait_for(lambda: abs(room() - base) <= unit // 2, timeout=20),
    )
