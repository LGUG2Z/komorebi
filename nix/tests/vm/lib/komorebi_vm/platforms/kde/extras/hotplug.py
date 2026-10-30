"""What only KDE can tell about monitors that change, round the shared `hotplug` scenario.

A scale that `kscreen-doctor` changes reaches the daemon as a change of the monitor's size; KWin
tells the daemon that a window is on another output when the daemon sent it there; and what the
daemon believes of its monitors is what the compositor has.

The suite runs `setup`, `scale` and `sent_by_the_daemon`, then the shared `user_move`, then the
shared `run`, which unplugs a display and plugs it in again, then `user_move` once more, then
`after`. The daemon runs on a session with two outputs, and `clients` are three windows on the
left one.
"""

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform


def _monitor(platform: KdePlatform, idx: int) -> Rect:
    """The bounds the daemon has for a monitor."""
    size = platform.kde.state()["monitors"]["elements"][idx]["size"]
    return Rect(size["left"], size["top"], size["right"], size["bottom"])


def _containers_on(platform: KdePlatform, idx: int) -> int:
    """How many containers the workspace in front of a monitor has in the daemon."""
    workspaces = platform.kde.state()["monitors"]["elements"][idx]["workspaces"]
    return len(workspaces["elements"][workspaces["focused"]]["containers"]["elements"])


def _rects(platform: KdePlatform, clients: list[Client]) -> list[Rect]:
    """The frames KWin has for the windows."""
    return [platform.kde.rect(platform.kde.wid(c.title)) for c in clients]


def _tiled_in(platform: KdePlatform, clients: list[Client], area: Rect) -> bool:
    """Every one of the windows is inside `area`, and none is on another.

    A pixel is allowed at each edge: at a fractional scale KWin reports frames that are not whole
    pixels.
    """
    frames = _rects(platform, clients)
    return all(r.inside(area, slack=1) for r in frames) and not any(
        a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]
    )


def _on_the_right(platform: KdePlatform, clients: list[Client]) -> list[Client]:
    """The windows that are on the right output, as KWin has them."""
    width = platform.size[0]
    pairs = zip(clients, _rects(platform, clients), strict=True)
    return [c for c, r in pairs if r.x >= width]


def _output_changes(platform: KdePlatform) -> int:
    """How many times KWin has told the daemon that a window is on another output."""
    events = daemon.events(platform.kde.komorebi_log())
    return sum(1 for e in events if e.kind == "OutputChange")


def setup(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The daemon has a monitor for each of the two outputs, with the size of its output, and the
    windows are on the left one."""
    width, height = platform.size
    checks.check(
        "the daemon answers komorebic, with both monitors",
        platform.wait_for(
            lambda: len(platform.state().monitors) == 2 and platform.komorebic("state") != "",
            timeout=30,
        ),
    )
    checks.check(
        "the daemon has the three windows on the left monitor",
        platform.wait_for(
            lambda: _containers_on(platform, 0) == 3 and _containers_on(platform, 1) == 0
        ),
    )
    checks.check(
        "the left monitor has the size of its output",
        _monitor(platform, 0) == Rect(0, 0, width, height),
    )
    checks.check(
        "the windows are tiled inside it",
        platform.wait_for(lambda: _tiled_in(platform, clients, _monitor(platform, 0))),
    )


def scale(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """The scale of the left output changes to 1.5 and back, and the daemon follows both."""
    width, height = platform.size
    left = platform.kde.outputs.names()[0]
    resizes = lambda: platform.kde.komorebi_log().count("updated monitor size and work area")  # noqa: E731
    before = resizes()
    platform.kde.outputs.scale(left, 1.5)
    smaller = Rect(0, 0, width * 2 // 3, height * 2 // 3)
    checks.check(
        f"the daemon has the new size of the monitor, {smaller}",
        platform.wait_for(lambda: _monitor(platform, 0) == smaller, timeout=20),
    )
    checks.check("it read the sizes again for it", resizes() > before)
    checks.check(
        "the windows are tiled again inside the monitor as it is now",
        platform.wait_for(lambda: _tiled_in(platform, clients, smaller)),
    )
    platform.kde.outputs.scale(left, 1)
    full = Rect(0, 0, width, height)
    checks.check(
        "the monitor has its size back, and the windows fill it again",
        platform.wait_for(
            lambda: _monitor(platform, 0) == full and _tiled_in(platform, clients, full),
            timeout=20,
        ),
    )


def sent_by_the_daemon(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """`move-to-monitor 1` sends the focused window to the right output: KWin reports it there,
    and the daemon leaves it where it put it."""
    changes = _output_changes(platform)
    platform.komorebic("move-to-monitor", "1")
    checks.check(
        "the daemon has two containers on the left monitor and one on the right",
        platform.wait_for(
            lambda: _containers_on(platform, 0) == 2 and _containers_on(platform, 1) == 1
        ),
    )
    checks.check(
        "KWin reported the window on the other output",
        platform.wait_for(lambda: _output_changes(platform) > changes),
    )
    moved = _on_the_right(platform, clients)
    checks.check(
        "the window is tiled on the right output",
        len(moved) == 1 and _tiled_in(platform, moved, _monitor(platform, 1)),
    )
    checks.check(
        "and it is still on the right monitor once KWin has said so",
        _containers_on(platform, 0) == 2 and _containers_on(platform, 1) == 1,
    )


def after(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """After the display was unplugged and plugged in again the daemon is alive and agrees with
    KWin on the focus. The shared `user_move` runs once more before it, for a window the user moves
    to the other output."""
    kde = platform.kde

    def focus_agrees() -> bool:
        # the focused window may be on either monitor: the shared scenario sent one to the other
        state = platform.state()
        return (
            kde.komorebi_running()
            and kde.active() != ""
            and kde.title(kde.active()) == state.focused_title(monitor=state.focused_monitor)
        )

    checks.check(
        "the daemon is alive and its idea of the focus is KWin's", platform.wait_for(focus_agrees)
    )
    checks.check(
        "the daemon is running and answers",
        kde.komorebi_running() and kde.run("komorebic state")[0] == 0,
    )
