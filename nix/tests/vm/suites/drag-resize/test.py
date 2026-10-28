# Resizing a tile by dragging its edge, in a Plasma session, with a pointer device the test drives
# from inside the guest (`komorebi_vm.Pointer`). Edges and corners of three tiles are grabbed and
# dragged, and what happened is checked against what a user expects
# (`komorebi/tests/drag_resize.rs` checks the window manager's side against the fake):
#
# * a dragged edge that has a neighbor moves by what the pointer moved, and the neighbor's edge
#   moves with it: they share it
# * a dragged edge with no neighbor (the edge of the screen) does not move, and the window is put
#   back
# * edges that were not dragged do not move, and every edge of every other tile either stays or
#   moves with a dragged one
# * the windows still tile: inside the screen, not overlapping
#
# The strict checks are on the tiles, which are komorebi's own layout (`latest_layout` in
# `komorebic state`). A terminal's frame is checked loosely: it sizes itself to whole cells of its
# font, so it is a few pixels short of its tile.
import re
import time

from komorebi_vm import Checks, Kde, Pointer, daemon

# the module's unit, which the session starts when it logs in: the daemon the suite drags against
UNIT = "komorebi.service"

# what an edge of a terminal's frame may be off by: up to a cell of its font
CELL = 24
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

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)

# KWin snaps an edge the user drags to an edge of another window within its snap zone, 10 pixels
# by default, and the neighbors that follow a drag are about that far from the edge: it would end
# up a few pixels past the pointer. The test measures komorebi, so KWin snaps nothing.
for zone in ("BorderSnapZone", "WindowSnapZone", "CenterSnapZone"):
    kde.sh(f"kwriteconfig6 --file kwinrc --group Windows --key {zone} 0")
kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin reconfigure")

# the session started the module's unit when it logged in
checks.check(
    "komorebi answers", kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=120)
)

# The windows tiled before the daemon has the panel's room are put in the room that was there, and a
# case that begins then ends with every tile moved by the panel. The wait is not a check: a session
# may never take the room, and it ends after a while.
kde.wait_for_panel_room()

size = kde.state()["monitors"]["elements"][0]["size"]
W, H = size["right"], size["bottom"]
pointer = Pointer.start(machine, (W, H), scale=SCALE)


def where() -> tuple[int, int]:
    found = re.search(r"x:(-?\d+) y:(-?\d+)", kde.sh("kdotool getmouselocation"))
    assert found, "kdotool says nowhere"
    return int(found.group(1)), int(found.group(2))


# the pointer goes where it is asked, to the pixel
worst = 0
for x, y in [(1, 1), (W - 2, 1), (W // 3, H // 2), (W - 2, H - 2), (17, H - 2)]:
    pointer.move(x, y)
    kde.wait_for(lambda x=x, y=y: where() == (x, y), timeout=3)
    gx, gy = where()
    worst = max(worst, abs(gx - x), abs(gy - y))
checks.check(f"the pointer is put within a pixel of where it is asked ({worst})", worst <= 1)

for n in range(1, WINDOWS + 1):
    kde.spawn(f"foot -T win{n} sleep 3000")
    kde.wait_for(lambda n=n: kde.managed() == n, timeout=30)
checks.check(f"the {WINDOWS} windows are tiled", kde.managed() == WINDOWS)

Box = tuple[int, int, int, int]


def frames() -> dict[str, Box]:
    out = {}
    for n in range(1, WINDOWS + 1):
        r = kde.rect(kde.wid(f"win{n}"))
        out[f"win{n}"] = (r.x, r.y, r.w, r.h)
    return out


def tiles() -> dict[str, Box]:
    """komorebi's layout: the tile it gave each window, by title"""
    workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
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


def settled() -> dict[str, Box]:
    """The frames once they have stopped changing"""
    last = frames()
    deadline = time.monotonic() + 10 * SCALE
    while time.monotonic() < deadline:
        time.sleep(0.5)
        now = frames()
        if now == last:
            return now
        last = now
    return last


def resize_ends() -> int:
    """How many drags of an edge the daemon has handled the end of, by the unit's log"""
    log = kde.run(f"journalctl --user -u {UNIT} --no-pager -o cat")[1]
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


def run_case(spec: str) -> list[str]:
    name, grab, d = spec.split(":")
    dx, dy = (int(v) for v in d.split(","))

    kde.komorebic("retile")
    before = settled()
    tiles_before = tiles()
    ends = resize_ends()

    px, py = press_point(grab, before[name])
    pointer.drag((px, py), (px + dx, py + dy))
    kde.wait_for(lambda: resize_ends() > ends, timeout=10)
    after = settled()
    tiles_after = tiles()

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


# BSP: win1 is the left half, win2 the top right and win3 the bottom right
cases = [
    "win1:right:72,0",
    "win1:right:-72,0",
    "win2:left:-72,0",
    "win2:left:-240,0",
    "win2:bottom:0,72",
    "win3:top:0,-72",
    "win2:bottomleft:-72,72",
    "win1:left:-72,0",
]
for spec in cases:
    problems = run_case(spec)
    checks.check(f"{spec}: " + ("as expected" if not problems else "; ".join(problems)), not problems)

checks.finish()
