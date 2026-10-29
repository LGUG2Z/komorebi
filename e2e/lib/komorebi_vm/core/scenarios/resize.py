"""Dragging the edge two tiles share resizes both, and nothing else moves."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform
from komorebi_vm.core.scenarios.layouts import tiled

# how far outside a window's frame its resize border is pressed by a platform whose border is the
# gap between two tiles (`outside_grab`)
OFFSET = 4
# the shortest drag, in pixels: far enough to be a drag on any display
MIN_DRAG = 60
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


def run(platform: Platform, checks: Checks, clients: list[Client], dx: int | None = None) -> None:
    """Drags the edge between a tile and the one to its right by `dx` pixels (a thirty-second of
    the work area's width, and at least `MIN_DRAG`, when it is not given), and checks that the
    two tiles moved the edge by what the pointer moved, that every other edge stayed, and that the
    windows still tile on the work area, as the operating system has them.

    `clients` are the tiled windows of the first workspace, each in a container of its own.
    """
    if not platform.has("pointer"):
        print("     skipped: this platform has no pointer")
        return
    if dx is None:
        dx = max(MIN_DRAG, platform.work_area().w // 32)
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
    grab = platform.resize_grab(window.frame, "right")

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


GRABS = {
    "left": "L",
    "right": "R",
    "top": "T",
    "bottom": "B",
    "topleft": "LT",
    "topright": "RT",
    "bottomleft": "LB",
    "bottomright": "RB",
}
# how far apart two windows may be for one to be the neighbour of the other: twice the gap
NEIGHBOUR = 80


def _edges(r: Rect) -> dict[str, int]:
    return {"L": r.x, "T": r.y, "R": r.x + r.w, "B": r.y + r.h}


def _tiles(platform: Platform) -> dict[str, Rect]:
    """The daemon's layout: the tile it gave each container, by the title of its window in view."""
    workspace = platform.state().workspace()
    return {
        c[min(workspace.focused_windows[i], len(c) - 1)].title: workspace.latest_layout[i]
        for i, c in enumerate(workspace.containers)
        if i < len(workspace.latest_layout) and c
    }


def _frames(platform: Platform, clients: list[Client]) -> dict[str, Rect]:
    return {c.title: w.frame for c in clients if (w := platform.os_window(c)) is not None}


def _settled(platform: Platform, clients: list[Client]) -> dict[str, Rect]:
    """The frames once they have stopped changing."""
    last = _frames(platform, clients)
    for _ in range(20):
        platform.wait_for(lambda: False, timeout=0.5)
        now = _frames(platform, clients)
        if now == last:
            return now
        last = now
    return last


def outside_grab(r: Rect, grab: str) -> tuple[int, int]:
    """Where the pointer is pressed for a grab by a platform whose resize border is the gap outside
    the frame, `OFFSET` pixels from it: what KDE has on every side."""
    left, right, top, bottom = r.x - OFFSET, r.x + r.w + OFFSET, r.y - OFFSET, r.y + r.h + OFFSET
    cx, cy = r.x + r.w // 2, r.y + r.h // 2
    return {
        "left": (left, cy),
        "right": (right, cy),
        "top": (cx, top),
        "bottom": (cx, bottom),
        "topleft": (left, top),
        "topright": (right, top),
        "bottomleft": (left, bottom),
        "bottomright": (right, bottom),
    }[grab]


def _has_neighbour(edge: str, name: str, frames: dict[str, Rect]) -> bool:
    """Is there another window within a gap's reach on that side of this one, overlapping it."""
    me = frames[name]
    for other, o in frames.items():
        if other == name:
            continue
        vertical = o.y < me.y + me.h and me.y < o.y + o.h
        horizontal = o.x < me.x + me.w and me.x < o.x + o.w
        if edge == "L" and vertical and 0 <= me.x - (o.x + o.w) <= NEIGHBOUR:
            return True
        if edge == "R" and vertical and 0 <= o.x - (me.x + me.w) <= NEIGHBOUR:
            return True
        if edge == "T" and horizontal and 0 <= me.y - (o.y + o.h) <= NEIGHBOUR:
            return True
        if edge == "B" and horizontal and 0 <= o.y - (me.y + me.h) <= NEIGHBOUR:
            return True
    return False


def _stacked(layout: list[Rect]) -> tuple[int, int] | None:
    """The first pair of tiles of which the second is below the first and overlaps it sideways."""
    for i, a in enumerate(layout):
        for j, b in enumerate(layout):
            if i != j and b.y >= a.y + a.h and a.x < b.x + b.w and b.x < a.x + a.w:
                return i, j
    return None


def _cases(
    platform: Platform, layout: list[Rect], titles: list[str], area: Rect
) -> list[tuple[str, str, int, int]]:
    """Drags to make, as (window, grab, dx, dy), from the layout: both sides of an edge that two
    tiles share, a horizontal one, a corner next to two neighbours, and an edge of the screen."""
    cases: list[tuple[str, str, int, int]] = []
    if (pair := _side_by_side(layout)) is not None:
        a, b = pair
        cases += [
            (titles[a], "right", 72, 0),
            (titles[a], "right", -72, 0),
            (titles[b], "left", -72, 0),
            (titles[b], "left", -240, 0),
        ]
    if (stack := _stacked(layout)) is not None:
        a, b = stack
        cases += [(titles[a], "bottom", 0, 72), (titles[b], "top", 0, -72)]
    for k, tile in enumerate(layout):
        right_of = any(
            i != k and tile.x >= t.x + t.w and t.y < tile.y + tile.h and tile.y < t.y + t.h
            for i, t in enumerate(layout)
        )
        above = any(
            i != k and t.y >= tile.y + tile.h and tile.x < t.x + t.w and t.x < tile.x + tile.w
            for i, t in enumerate(layout)
        )
        if right_of and above:
            cases.append((titles[k], "bottomleft", -72, 72))
            break
    first = min(range(len(layout)), key=lambda i: layout[i].x)
    if platform.resize_grab(layout[first], "left")[0] >= area.x:
        cases.append((titles[first], "left", -72, 0))
    return cases


def _problems(
    platform: Platform,
    clients: list[Client],
    case: tuple[str, str, int, int],
) -> list[str]:
    """Makes a drag and says what is wrong with the outcome: the frame the user sees, the layout,
    the edges that were not dragged and the windows."""
    name, grab, dx, dy = case
    area = platform.work_area()
    platform.komorebic("retile")
    # the frames follow the tiles a moment later, and a press on a frame that is still on its way
    # lands on the window and not on its border (a drop on another tile, a swap)
    platform.wait_for(lambda: tiled(platform, clients), timeout=10)
    before = _settled(platform, clients)
    tiles_before = _tiles(platform)

    px, py = platform.resize_grab(before[name], grab)
    end = (max(area.x, px + dx), max(area.y, py + dy))
    platform.pointer.drag((px, py), end)
    platform.wait_for(lambda: _tiles(platform) != tiles_before, timeout=3)
    # a neighbour's frame follows its tile a moment later; it is read when it has
    platform.wait_for(lambda: tiled(platform, clients), timeout=10)
    after = _settled(platform, clients)
    tiles_after = _tiles(platform)

    held = GRABS[grab]
    want = {"L": dx, "R": dx, "T": dy, "B": dy}
    problems = []
    moved = {e: _edges(after[name])[e] - _edges(before[name])[e] for e in "LTRB"}
    for e in "LTRB":
        if e in held and _has_neighbour(e, name, before):
            if abs(moved[e] - want[e]) > platform.tolerance:
                problems.append(f"frame {e} moved {moved[e]}, the pointer moved {want[e]}")
        elif abs(moved[e]) > platform.tolerance:
            why = "was not dragged" if e not in held else "has no neighbour"
            problems.append(f"frame {e} moved {moved[e]} but {why}")

    tile_moved = {e: _edges(tiles_after[name])[e] - _edges(tiles_before[name])[e] for e in "LTRB"}
    for e in "LTRB":
        if e in held and _has_neighbour(e, name, before):
            if abs(tile_moved[e] - want[e]) > 2:
                problems.append(f"tile {e} moved {tile_moved[e]}, the pointer moved {want[e]}")
        elif abs(tile_moved[e]) > 1:
            why = "was not dragged" if e not in held else "has no neighbour"
            problems.append(f"tile {e} moved {tile_moved[e]} but {why}")

    # nothing else moves, but with a dragged edge: they share it
    follows = [m for m in tile_moved.values() if abs(m) > 1]
    for other in tiles_after:
        if other == name:
            continue
        for e in "LTRB":
            m = _edges(tiles_after[other])[e] - _edges(tiles_before[other])[e]
            if abs(m) > 1 and not any(abs(m - f) <= 2 for f in follows):
                problems.append(f"{other} tile {e} moved {m}: not with the dragged edge")

    names = list(after)
    for i, a in enumerate(names):
        if not after[a].inside(area):
            problems.append(f"{a} {after[a]} is outside the work area")
        problems += [f"{a} and {b} overlap" for b in names[i + 1 :] if after[a].overlaps(after[b])]
    print(f"     {case}: frame {before[name]} -> {after[name]}, tile edges {tile_moved}")
    return problems


def edge_cases(platform: Platform, checks: Checks, clients: list[Client]) -> None:
    """Drags both sides of an edge two tiles share, a horizontal edge, a corner and an edge of the
    screen, and checks, against the layout the daemon computed (`latest_layout`) and the frames the
    operating system has:

    - a dragged edge that has a neighbour moves by what the pointer moved, and the neighbour's
      edge moves with it
    - a dragged edge with no neighbour does not move, and the window is put back
    - edges that were not dragged do not move, and every edge of another tile stays or moves with
      a dragged one
    - the windows still tile: inside the work area, not on each other

    `clients` are the tiled windows of the first workspace, each in a container of its own, in a
    layout that has them side by side or stacked. The cases are made from the layout, so that
    a layout with no stacked tiles has no horizontal case. The layout is retiled before each one.
    """
    if not platform.has("pointer"):
        print("     skipped: this platform has no pointer")
        return
    platform.komorebic("retile")
    platform.wait_for(lambda: len(_tiles(platform)) == len(clients), timeout=20)
    tiles = _tiles(platform)
    titles = list(tiles)
    layout = [tiles[t] for t in titles]
    for case in _cases(platform, layout, titles, platform.work_area()):
        problems = _problems(platform, clients, case)
        name, grab, dx, dy = case
        checks.check(
            f"{name}:{grab}:{dx},{dy}: " + ("as expected" if not problems else "; ".join(problems)),
            not problems,
        )
