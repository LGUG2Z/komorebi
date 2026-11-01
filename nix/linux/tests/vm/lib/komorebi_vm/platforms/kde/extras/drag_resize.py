"""What only KDE can tell about dragging with a real pointer.

The pointer is a device made inside the guest. The shared `resize` makes the drags of the edges and
corners of the tiles, against the daemon's layout and the frames the operating system has
(`komorebi/tests/drag_resize.rs` checks the window manager's side against the fake); what is here is
that the device puts the pointer to the pixel, which a drag at a gap of four pixels depends on.

The suite runs `pointer_is_exact` on a session with a pointer and three tiled windows, `win1` to
`win3`, then the shared `resize.edge_cases`, `resize.run` and `focus_follows_mouse`.
"""

import re

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform

WINDOWS = 3


def _where(platform: KdePlatform) -> tuple[int, int]:
    found = re.search(r"x:(-?\d+) y:(-?\d+)", platform.kde.sh("kdotool getmouselocation"))
    assert found, "kdotool says nowhere"
    return int(found.group(1)), int(found.group(2))


def pointer_is_exact(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The pointer goes where it is asked, to the pixel."""
    width, height = platform.size
    worst = 0
    for x, y in [(1, 1), (width - 2, 1), (width // 3, height // 2), (width - 2, height - 2)] + [
        (17, height - 2)
    ]:
        platform.pointer.move(x, y)
        platform.kde.wait_for(lambda x=x, y=y: _where(platform) == (x, y), timeout=3)
        gx, gy = _where(platform)
        worst = max(worst, abs(gx - x), abs(gy - y))
    checks.check(f"the pointer is put within a pixel of where it is asked ({worst})", worst <= 1)
