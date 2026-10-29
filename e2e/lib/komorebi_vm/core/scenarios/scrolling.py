"""In the scrolling layout the window that has the focus is in view, wherever the others are.

`run` checks that on the windows a suite has open. `columns` opens five windows of its own in two
columns, and checks where the strip puts the ones that are out of view, that it scrolls with the
focus and with `pan`, and the commands of the columns (`consume`, `expel`, `toggle-column-stack`,
`scrolling-layout-full-width`). Both ports lay the strip out with the same model.
"""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled
from komorebi_vm.core.scenarios.monitors import _plug
from komorebi_vm.core.scenarios.support import focus_agrees

STRIP = 5

# how many pixels of a window that rests at an edge of the work area are on the screen: 5 of them by
# design, and the paddings, what the compositor rounds and a terminal's grid of cells are the rest
PARKED_AT_MOST = 30


def run(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Changes the layout of the first workspace, which has `clients` tiled in a container each,
    to scrolling, gives each of them the focus in turn, and changes it back to bsp."""
    area = platform.work_area()

    def in_view(client: Client) -> bool:
        window = platform.os_window(client)
        return window is not None and window.frame.inside(area)

    platform.komorebic("change-layout", "scrolling")
    for client in clients:
        platform.activate(client)
        checks.check(
            f"in scrolling, {client.title} has the focus and is inside the work area",
            platform.wait_for(
                lambda c=client: focus_agrees(platform, clients, c.title) and in_view(c),
                timeout=20,
            ),
        )

    platform.komorebic("change-layout", "bsp")
    checks.check(
        "back in bsp every window is where the layout says",
        platform.wait_for(lambda: tiled(platform, clients), timeout=20),
    )


def columns(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Five windows in two columns on a workspace with none: the last two are in view and the
    others are scrolled out, where the system keeps them; focusing to the far left and back, and
    `pan`, scroll the strip; then the commands of a column of several windows. The layout is bsp
    again and the windows are closed at the end."""
    area = platform.work_area()
    left, right = area.x, area.x + area.w
    tol = platform.tolerance
    titles = [f"strip{i}" for i in range(1, STRIP + 1)]

    def frame(i: int) -> Rect:
        window = platform.os_window(Client(titles[i - 1]))
        return window.frame if window is not None else Rect(0, 0, 0, 0)

    def in_view(i: int) -> bool:
        r = frame(i)
        return r.x >= left - tol and r.x + r.w <= right + tol

    def out_of_view(i: int) -> bool:
        r = frame(i)
        return r.x >= right - PARKED_AT_MOST or r.x + r.w <= left + PARKED_AT_MOST

    def agree(i: int) -> bool:
        return focus_agrees(platform, [Client(t) for t in titles], titles[i - 1])

    def wait(predicate, timeout: float = 10) -> bool:  # noqa: ANN001
        return platform.wait_for(predicate, timeout=timeout)

    platform.komorebic("change-layout", "scrolling")
    platform.komorebic("scrolling-layout-columns", "2")
    opened = [platform.spawn(t) for t in titles]
    checks.check(
        "the daemon manages all five", wait(lambda: platform.state().managed() >= STRIP, 20)
    )
    checks.check(
        "the focused (last) window is in view, and so is the one before it",
        wait(lambda: in_view(5) and in_view(4), 20),
    )
    checks.check(
        "the first three are scrolled out of view",
        wait(lambda: all(out_of_view(i) for i in (1, 2, 3)), 20),
    )
    # the window rests with its right edge a few pixels inside the work area, as wide as the one
    # in view: nothing pulled it further onto the screen, and nothing was resized. A terminal draws
    # in whole cells, so its frame can be a cell narrower than the tile it was rested by
    checks.check(
        "the first window rests at the left edge, a few pixels of it on the screen or a cell short"
        f" of them (right edge at {frame(1).x + frame(1).w}), not pulled back",
        wait(lambda: left - tol < frame(1).x + frame(1).w <= left + PARKED_AT_MOST),
    )
    checks.check(
        f"... and it kept its width ({frame(1).w} against {frame(4).w})",
        wait(lambda: abs(frame(1).w - frame(4).w) <= tol),
    )
    checks.check("the system and the daemon agree on the focus", wait(lambda: agree(5)))

    for _ in range(STRIP - 1):
        platform.komorebic("focus", "left")
    checks.check(
        "focusing to the far left scrolls the strip back: the first is in view",
        wait(lambda: in_view(1) and agree(1)),
    )
    checks.check("... and the last is past the right edge", wait(lambda: out_of_view(5)))
    for _ in range(STRIP - 1):
        platform.komorebic("focus", "right")
    checks.check(
        "walking right again reaches the last, in view", wait(lambda: agree(5) and in_view(5))
    )

    # the view moves a column without the focus moving first
    platform.komorebic("pan", "left")
    checks.check(
        "panning left brings the third into view and puts the last out of it",
        wait(lambda: in_view(3) and out_of_view(5)),
    )
    checks.check(
        "... and the focus goes to the nearest column in view, the fourth", wait(lambda: agree(4))
    )
    platform.komorebic("pan", "right")
    checks.check("panning right puts the last in view again", wait(lambda: in_view(5)))

    # a column of two windows: the fourth is put at the top of the column of the fifth
    platform.komorebic("focus", "right")
    wait(lambda: agree(5))
    platform.komorebic("consume", "left")
    checks.check(
        "consuming left puts the fourth above the fifth, in one column",
        wait(lambda: abs(frame(4).x - frame(5).x) <= tol and frame(4).y < frame(5).y),
    )
    checks.check(
        "... each with a share of the height of the column",
        wait(lambda: frame(4).h < area.h * 0.6 and frame(5).h < area.h * 0.6),
    )
    checks.check("... and the focus stays on the fifth", wait(lambda: agree(5)))

    pair = [Client(titles[3]), Client(titles[4])]
    platform.komorebic("toggle-column-stack")
    checks.check(
        "stacking the column shows one window of the two",
        wait(lambda: sum(platform.hidden(c) for c in pair) == 1),
    )
    platform.komorebic("toggle-column-stack")
    checks.check(
        "spreading it out shows both again, one above the other",
        wait(lambda: not any(platform.hidden(c) for c in pair) and frame(4).y < frame(5).y),
    )

    platform.komorebic("expel")
    checks.check(
        "expelling puts the focused window in a column of its own on the right",
        wait(lambda: frame(5).x > frame(4).x + tol and abs(frame(5).y - frame(4).y) <= tol),
    )

    platform.komorebic("scrolling-layout-full-width")
    checks.check(
        "the full width makes the focused column as wide as the work area",
        wait(lambda: frame(5).w >= area.w - 60 and in_view(5)),
    )
    platform.komorebic("scrolling-layout-full-width")
    checks.check(
        "and again puts it back to the width it had",
        wait(lambda: abs(frame(5).w - frame(4).w) <= tol),
    )

    platform.komorebic("change-layout", "bsp")
    for client in opened:
        platform.close(client)


def two_monitors(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """A strip with a display next to it leaks no window onto it.

    Five windows in two columns on the first monitor, with a second display on its right. A window
    shows on whichever display it overlaps, so one that rests past the right edge, or overhangs it,
    would be seen on the neighbour: the daemon hides it, or makes it narrower, instead. Scrolling
    with `focus` and `pan`, both ways, leaves no shown window on the second display, and no window
    that is shown past the right edge of the first. The layout is bsp again, the windows are
    closed, and the display is unplugged if the scenario plugged it, at the end."""
    plugged = _plug(platform)
    first, second = platform.displays()[0], platform.displays()[1]
    tol = platform.tolerance
    titles = [f"leak{i}" for i in range(1, STRIP + 1)]
    mine = [Client(t) for t in titles]

    def leaks() -> list[str]:
        """The windows that are shown and have a part on the second display, or reach past the
        right edge of the first."""
        out = []
        for client in mine:
            window = platform.os_window(client)
            if window is None or platform.hidden(client):
                continue
            frame = window.frame
            if frame.overlaps(second) or frame.x + frame.w > first.x + first.w + tol:
                out.append(f"{client.title} {frame}")
        return out

    def settled(title: str) -> bool:
        return focus_agrees(platform, mine, title)

    def quiet(step: str, title: str) -> None:
        platform.wait_for(lambda: settled(title), timeout=10)
        platform.wait_for(lambda: not leaks(), timeout=5)
        checks.check(f"{step}: no window is shown on the second display", not leaks())
        if leaks():
            print("     leaked: " + "; ".join(leaks()))

    platform.komorebic("focus-monitor", "0")
    platform.komorebic("change-layout", "scrolling")
    platform.komorebic("scrolling-layout-columns", "2")
    opened = [platform.spawn(t) for t in titles]
    checks.check(
        "the daemon manages all five",
        platform.wait_for(lambda: platform.state().managed() >= STRIP, timeout=20),
    )
    quiet("with the strip at its right end", titles[-1])
    for i in range(STRIP - 1, 0, -1):
        platform.komorebic("focus", "left")
        quiet(f"focusing left to {titles[i - 1]}", titles[i - 1])
    for i in range(1, STRIP):
        platform.komorebic("focus", "right")
        quiet(f"focusing right to {titles[i]}", titles[i])
    platform.komorebic("pan", "left")
    platform.wait_for(lambda: not leaks(), timeout=10)
    checks.check("panning left: no window is shown on the second display", not leaks())
    platform.komorebic("pan", "right")
    platform.wait_for(lambda: not leaks(), timeout=10)
    checks.check("panning right: no window is shown on the second display", not leaks())

    platform.komorebic("change-layout", "bsp")
    for client in opened:
        platform.close(client)
    platform.komorebic("focus-workspace", "0")
    if plugged:
        platform.remove_display()
        platform.wait_for(
            lambda: len(platform.state().monitors) == len(platform.displays()), timeout=30
        )
