"""What kfw adds to the padding its state names: the room of the border."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client

from ..windows_platform import WindowsPlatform

# kfw pads a window by the width of its border plus the border's offset when
# borders are on, which they are by default (border_manager: 8 and -1)
BORDER_ROOM = 8 - 1


def run(platform: WindowsPlatform, checks: Checks, clients: list[Client]) -> None:
    """Measures how far the tiled windows of the first workspace are from the
    edges of the work area and from each other, against the paddings the
    state gives. The daemon has `clients` tiled, with its default padding."""
    workspace = platform.state().workspace()
    padding = workspace.workspace_padding
    container = workspace.container_padding
    if padding is None or container is None:
        checks.skip("the state gives the paddings", "it does not")
        return
    area = platform.work_area()
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    inset = padding + container + BORDER_ROOM
    edges = (
        min(f.x for f in frames) - area.x,
        min(f.y for f in frames) - area.y,
        area.x + area.w - max(f.x + f.w for f in frames),
        area.y + area.h - max(f.y + f.h for f in frames),
    )
    print(f"     paddings {padding}/{container}, edges {edges}")
    checks.check(
        f"the windows are {inset} px from every edge: both paddings and "
        f"{BORDER_ROOM} px for the border",
        all(edge == inset for edge in edges),
    )
    first = min(frames, key=lambda f: f.x)
    neighbours = [f for f in frames if f.x > first.x and f.y < first.y + first.h
                  and f.y + f.h > first.y]
    if neighbours:
        gap = min(f.x for f in neighbours) - (first.x + first.w)
        checks.check(
            f"two windows side by side are {2 * (container + BORDER_ROOM)} px apart",
            gap == 2 * (container + BORDER_ROOM),
        )
