"""What only KDE can tell about the scrolling layout, beside the shared `scrolling` scenario.

The strip runs past the edge of the work area, and KWin is expected to leave the windows exactly
where the daemon puts them, off the screen too: KWin keeps part of a window it resizes on an
output, and the bridge asks again for the position of a window that was pulled back (`settle` in
`bridge.js`). The shared scenario checks that the focused window is in view; these check where
KWin keeps the ones that are not, and that the strip scrolls with the focus `komorebic focus`
moves.
"""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform

WINDOWS = 5


def run(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Five windows in two columns: the last two are in view and the others are scrolled out, where
    KWin keeps them; focusing to the far left and back scrolls the strip with the focus. The daemon
    must be running with no window. The daemon is stopped at the end."""
    kde = platform.kde
    width = platform.size[0]

    def n(i: int) -> Rect:
        return kde.rect(kde.wid(f"win{i}"))

    def in_view(r: Rect) -> bool:
        return r.x >= 0 and r.x + r.w <= width

    def out_of_view(r: Rect) -> bool:
        return r.x >= width or r.x + r.w <= 0

    def focused_monitor() -> int:
        return platform.state().focused_monitor

    def agree(title: str) -> bool:
        return kde.title(kde.active()) == title and kde.focused_title() == title

    kde.komorebic("change-layout", "scrolling")
    kde.komorebic("scrolling-layout-columns", "2")
    for i in range(1, WINDOWS + 1):
        platform.spawn(f"win{i}")
    checks.check(
        "the daemon manages all five", kde.wait_for(lambda: kde.managed() == WINDOWS, timeout=20)
    )
    checks.check(
        "the focused (last) window is in view, and so is the one before it",
        kde.wait_for(lambda: in_view(n(5)) and in_view(n(4)), timeout=20),
    )
    checks.check(
        "the first three are scrolled out of view",
        kde.wait_for(lambda: all(out_of_view(n(i)) for i in (1, 2, 3)), timeout=20),
    )
    for i in range(1, WINDOWS + 1):
        print(f"     win{i}: {n(i)}")
    # not an absolute x: the paddings move every column
    pitch = n(5).x - n(4).x
    checks.check(
        f"KWin left them where the daemon put them: win1 is three columns left of win4"
        f" ({n(1).x} = {n(4).x} - 3 x {pitch}), not pulled back",
        kde.wait_for(lambda: n(1).x == n(4).x - 3 * pitch, timeout=10),
    )
    checks.check("KWin and the daemon agree on the focus", kde.wait_for(lambda: agree("win5")))

    for _ in range(4):
        kde.komorebic("focus", "left")
        kde.wait_for(lambda: kde.title(kde.active()) == kde.focused_title())
    checks.check(
        "focusing to the far left scrolls the strip back: win1 in view",
        kde.wait_for(lambda: in_view(n(1)), timeout=10),
    )
    checks.check("... and win5 is past the right edge", kde.wait_for(lambda: out_of_view(n(5))))
    checks.check("... and the focus is win1 on both sides", kde.wait_for(lambda: agree("win1")))
    checks.check("... on the first monitor", focused_monitor() == 0)

    for _ in range(4):
        kde.komorebic("focus", "right")
        kde.wait_for(lambda: kde.title(kde.active()) == kde.focused_title())
    checks.check("walking right again reaches win5", kde.wait_for(lambda: agree("win5")))
    checks.check("... without the focus having left the first monitor", focused_monitor() == 0)
    checks.check("... and win5 is in view", kde.wait_for(lambda: in_view(n(5)), timeout=10))

    kde.komorebic("stop")
    kde.wait_for(lambda: not kde.komorebi_running())
