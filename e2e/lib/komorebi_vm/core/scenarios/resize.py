"""Dragging the edge two tiles share resizes both, and nothing else moves."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform

# how far outside a window's frame its resize border is pressed: the gap between two tiles
OFFSET = 4
# how far the edge may end up from where the pointer put it, in pixels
SLACK = 6


def _side_by_side(layout: list[Rect]) -> tuple[int, int] | None:
    """The first pair of tiles of which the second is to the right of the first, with the same top,
    so that the edge between them is one the pointer can take."""
    for i, a in enumerate(layout):
        for j, b in enumerate(layout):
            if i != j and b.x >= a.x + a.w and a.y < b.y + b.h and b.y < a.y + a.h:
                return i, j
    return None


def run(platform: Platform, checks: Checks, clients: list[Client], dx: int = 60) -> None:
    """Drags the edge between a tile and the one to its right by `dx` pixels, and checks that the
    two tiles moved the edge by what the pointer moved, that every other edge stayed, and that the
    windows still tile on the work area, as the operating system has them.

    `clients` are the tiled windows of the first workspace, each in a container of its own.
    """
    if not platform.has("pointer"):
        print("     skipped: this platform has no pointer")
        return
    state = platform.state()
    workspace = state.workspace()
    layout = workspace.latest_layout
    pair = _side_by_side(layout)
    checks.check("two tiles share a vertical edge", pair is not None)
    if pair is None:
        return
    left, right = pair
    titles = [c[0].title for c in workspace.containers]
    window = platform.os_window(Client(titles[left]))
    assert window is not None
    before = list(layout)
    grab = (window.frame.x + window.frame.w + OFFSET, window.frame.y + window.frame.h // 2)

    platform.pointer.drag(grab, (grab[0] + dx, grab[1]))

    def edge_moved() -> bool:
        now = platform.state().workspace().latest_layout
        return len(now) == len(before) and abs(now[left].w - (before[left].w + dx)) <= SLACK

    checks.check(
        f"the tile on the left of the edge is {dx} pixels wider",
        platform.wait_for(edge_moved, timeout=20),
    )
    now = platform.state().workspace().latest_layout
    print(f"     tiles before {before}, after {now}")
    edge = before[left].x + before[left].w
    # the tiles on either side of the edge: every one that ends at it, and every one that starts
    # at or after it with the edge as its own left side (a column of tiles shares it)
    ends = [i for i, t in enumerate(before) if t.x + t.w == edge]
    starts = [i for i, t in enumerate(before) if t.x == before[right].x]
    checks.check(
        f"the tiles that end at the edge are {dx} pixels wider and start where they did",
        all(abs(now[i].w - (before[i].w + dx)) <= SLACK and now[i].x == before[i].x for i in ends),
    )
    checks.check(
        f"the tiles that start after the edge start {dx} pixels later and end where they did",
        all(
            abs(now[i].x - (before[i].x + dx)) <= SLACK
            and abs((now[i].x + now[i].w) - (before[i].x + before[i].w)) <= SLACK
            for i in starts
        ),
    )
    checks.check(
        "no tile moved up or down or changed its height",
        all((now[i].y, now[i].h) == (before[i].y, before[i].h) for i in range(len(before))),
    )
    checks.check(
        "every tile that does not touch the edge is where it was",
        all(now[i] == before[i] for i in range(len(before)) if i not in ends and i not in starts),
    )
    area = platform.work_area()
    frames = [w.frame for c in clients if (w := platform.os_window(c)) is not None]
    checks.check("the windows are inside the work area", all(f.inside(area) for f in frames))
    checks.check(
        "and do not overlap",
        not any(a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]),
    )
