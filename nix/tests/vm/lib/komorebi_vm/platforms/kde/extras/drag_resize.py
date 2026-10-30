"""What only KDE can tell about dragging the edges of tiles with a real pointer.

The pointer is a device made inside the guest, and KWin gives the daemon the end of each drag. Edges
and corners of three tiles are grabbed and dragged, and what happened is checked against what a
user expects (`komorebi/tests/drag_resize.rs` checks the window manager's side against the fake):

* a dragged edge that has a neighbor moves by what the pointer moved, and the neighbor's edge moves
  with it: they share it
* a dragged edge with no neighbor (the edge of the screen) does not move, and the window is put back
* edges that were not dragged do not move, and every edge of every other tile either stays or moves
  with a dragged one
* the windows still tile: inside the screen, not overlapping

The strict checks are on the tiles, which are the daemon's own layout (`latest_layout` in
`komorebic state`). A terminal's frame is checked loosely: it sizes itself to whole cells of its
font, so it is a few pixels short of its tile. The shared `resize` makes one drag and reads the
operating system; these are the cases it leaves out.

The suite runs `pointer_is_exact` and `edge_cases` on a session with a pointer and three tiled
windows, `win1` to `win3`, then the shared scenarios.
"""

import re
import time

from komorebi_vm import daemon
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import CELL, KdePlatform

# the module's unit, which the session starts when it logs in: the daemon the suite drags against
UNIT = "komorebi.service"

# how far from its frame a window's resize border is pressed: in the gap between two tiles
OFFSET = 4
WINDOWS = 3

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

# BSP: win1 is the left half, win2 the top right and win3 the bottom right
CASES = [
    "win1:right:72,0",
    "win1:right:-72,0",
    "win2:left:-72,0",
    "win2:left:-240,0",
    "win2:bottom:0,72",
    "win3:top:0,-72",
    "win2:bottomleft:-72,72",
    "win1:left:-72,0",
]

Box = tuple[int, int, int, int]


def frames(platform: KdePlatform) -> dict[str, Box]:
    kde = platform.kde
    out = {}
    for n in range(1, WINDOWS + 1):
        r = kde.rect(kde.wid(f"win{n}"))
        out[f"win{n}"] = (r.x, r.y, r.w, r.h)
    return out


def tiles(platform: KdePlatform) -> dict[str, Box]:
    """komorebi's layout: the tile it gave each window, by title"""
    workspace = platform.kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
    out = {}
    for i, container in enumerate(workspace["containers"]["elements"]):
        windows = container["windows"]
        title = windows["elements"][windows["focused"]]["details"]["title"]
        tile = workspace["latest_layout"][i]
        out[title] = (tile["left"], tile["top"], tile["right"], tile["bottom"])
    return out


def edges(r: Box) -> dict[str, int]:
    x, y, w, h = r
    return {"L": x, "T": y, "R": x + w, "B": y + h}


def settled(platform: KdePlatform) -> dict[str, Box]:
    """The frames once they have stopped changing"""
    last = frames(platform)
    deadline = time.monotonic() + 10 * platform.kde.scale
    while time.monotonic() < deadline:
        time.sleep(0.5)
        now = frames(platform)
        if now == last:
            return now
        last = now
    return last


def resize_ends(platform: KdePlatform) -> int:
    """How many drags of an edge the daemon has handled the end of, by the unit's log"""
    log = platform.kde.run(f"journalctl --user -u {UNIT} --no-pager -o cat")[1]
    return sum(1 for e in daemon.events(log) if e.kind == "ResizeEnd")


def press_point(grab: str, r: Box) -> tuple[int, int]:
    x, y, w, h = r
    left, right, top, bottom = x - OFFSET, x + w + OFFSET, y - OFFSET, y + h + OFFSET
    cx, cy = x + w // 2, y + h // 2
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


def has_neighbor(edge: str, name: str, before: dict[str, Box]) -> bool:
    """Is there another window within a gap's reach on that side of this one, overlapping it"""
    mx, my, mw, mh = before[name]
    for other, (ox, oy, ow, oh) in before.items():
        if other == name:
            continue
        vertical_overlap = oy < my + mh and my < oy + oh
        horizontal_overlap = ox < mx + mw and mx < ox + ow
        if edge == "L" and vertical_overlap and 0 <= mx - (ox + ow) <= 80:
            return True
        if edge == "R" and vertical_overlap and 0 <= ox - (mx + mw) <= 80:
            return True
        if edge == "T" and horizontal_overlap and 0 <= my - (oy + oh) <= 80:
            return True
        if edge == "B" and horizontal_overlap and 0 <= oy - (my + mh) <= 80:
            return True
    return False


def run_case(platform: KdePlatform, spec: str) -> list[str]:
    kde = platform.kde
    W, H = platform.size
    name, grab, d = spec.split(":")
    dx, dy = (int(v) for v in d.split(","))

    kde.komorebic("retile")
    before = settled(platform)
    tiles_before = tiles(platform)
    ends = resize_ends(platform)

    px, py = press_point(grab, before[name])
    platform.pointer.drag((px, py), (px + dx, py + dy))
    kde.wait_for(lambda: resize_ends(platform) > ends, timeout=10)
    after = settled(platform)
    tiles_after = tiles(platform)

    held = GRABS[grab]
    want = {"L": dx, "R": dx, "T": dy, "B": dy}
    problems = []

    # the frame the user sees: it did what was asked, give or take the client's cells
    moved = {e: edges(after[name])[e] - edges(before[name])[e] for e in "LTRB"}
    for e in "LTRB":
        if e in held and has_neighbor(e, name, before):
            if abs(moved[e] - want[e]) > CELL:
                problems.append(f"frame {e} moved {moved[e]}, the pointer moved {want[e]}")
        elif abs(moved[e]) > CELL:
            why = "was not dragged" if e not in held else "has no neighbor"
            problems.append(f"frame {e} moved {moved[e]} but {why}")

    # the layout, which is exact
    tile_moved = {e: edges(tiles_after[name])[e] - edges(tiles_before[name])[e] for e in "LTRB"}
    for e in "LTRB":
        if e in held and has_neighbor(e, name, before):
            if abs(tile_moved[e] - want[e]) > 2:
                problems.append(f"tile {e} moved {tile_moved[e]}, the pointer moved {want[e]}")
        elif abs(tile_moved[e]) > 1:
            why = "was not dragged" if e not in held else "has no neighbor"
            problems.append(f"tile {e} moved {tile_moved[e]} but {why}")

    # nothing else moves, but with a dragged edge: they share it
    follows = [m for m in tile_moved.values() if abs(m) > 1]
    for other in tiles_after:
        if other == name:
            continue
        for e in "LTRB":
            m = edges(tiles_after[other])[e] - edges(tiles_before[other])[e]
            if abs(m) > 1 and not any(abs(m - f) <= 2 for f in follows):
                problems.append(f"{other} tile {e} moved {m}: not with the dragged edge")

    # still a tiling
    names = list(after)
    for i, n1 in enumerate(names):
        x, y, w, h = after[n1]
        if x < 0 or y < 0 or x + w > W or y + h > H:
            problems.append(f"{n1} {after[n1]} is off the screen")
        for n2 in names[i + 1 :]:
            ox, oy, ow, oh = after[n2]
            if x < ox + ow and ox < x + w and y < oy + oh and oy < y + h:
                problems.append(f"{n1} and {n2} overlap")

    print(f"{spec}: frame {before[name]} -> {after[name]}, tile edges {tile_moved}")
    return problems


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


def edge_cases(platform: KdePlatform, checks: Checks, clients: list[Client]) -> None:
    """Drags each edge and corner of `CASES` and checks the frames, the tiles and the neighbors."""
    for spec in CASES:
        problems = run_case(platform, spec)
        checks.check(
            f"{spec}: " + ("as expected" if not problems else "; ".join(problems)),
            not problems,
        )
