# The tiles next to an edge follow it while it is dragged, not only when it is let go, in a Plasma
# session with a pointer device the test drives from inside the guest (`komorebi_vm.Pointer`). For
# each case the button is pressed on an edge of a window, the pointer is taken part of the way and
# held there, and the frames of every window are read while the button is still down; then the
# rest of the way, read again, and let go (`komorebi/tests/live_resize.rs` checks the window
# manager's side against the fake). What only a real KWin can tell is that the bridge sends the
# steps of a drag and that the neighbors are moved while KWin has the dragged window:
#
# * part way and all the way, the neighbor's facing edge has moved with the pointer, give or take
#   a cell of the terminal's font
# * the windows that are not next to the dragged edge have not moved
# * when the button is let go nothing jumps: the neighbors stay where they followed to
# * the windows still tile
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

OPPOSITE = {"L": "R", "R": "L", "T": "B", "B": "T"}
HELD = {"left": "L", "right": "R", "top": "T", "bottom": "B"}

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

# The shell makes its panel, and KWin takes room for it a while later, which the daemon learns of
# up to a second after that and tiles the windows again. A case that begins then ends with every
# tile moved by the panel, so the windows are opened after it (`Kde.wait_for_panel_room`).
kde.wait_for_panel_room()

size = kde.state()["monitors"]["elements"][0]["size"]
W, H = size["right"], size["bottom"]
pointer = Pointer.start(machine, (W, H), scale=SCALE)

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
    cx, cy = x + w // 2, y + h // 2
    return {
        "left": (x - OFFSET, cy),
        "right": (x + w + OFFSET, cy),
        "top": (cx, y - OFFSET),
        "bottom": (cx, y + h + OFFSET),
    }[grab]


def neighbors(edge: str, name: str, before: dict[str, Box]) -> list[str]:
    """The windows within a gap's reach on that side of this one, overlapping it"""
    mx, my, mw, mh = before[name]
    found = []
    for other, (ox, oy, ow, oh) in before.items():
        if other == name:
            continue
        v = oy < my + mh and my < oy + oh
        h = ox < mx + mw and mx < ox + ow
        if (
            (edge == "L" and v and 0 <= mx - (ox + ow) <= 80)
            or (edge == "R" and v and 0 <= ox - (mx + mw) <= 80)
            or (edge == "T" and h and 0 <= my - (oy + oh) <= 80)
            or (edge == "B" and h and 0 <= oy - (my + mh) <= 80)
        ):
            found.append(other)
    return found


def where() -> tuple[int, int]:
    found = re.search(r"x:(-?\d+) y:(-?\d+)", kde.sh("kdotool getmouselocation"))
    assert found, "kdotool says nowhere"
    return int(found.group(1)), int(found.group(2))


def run_case(case: str) -> list[str]:
    name, grab, d = case.split(":")
    dx, dy = (int(v) for v in d.split(","))
    held = HELD[grab]
    want = dx if held in "LR" else dy

    kde.komorebic("retile")
    before = settled()
    near = neighbors(held, name, before)
    if not near:
        return [f"{name} has nothing next to its {grab} edge"]
    ends = resize_ends()

    px, py = press_point(grab, before[name])
    pointer.move(px, py)
    pointer.down()

    problems = []
    held_at: dict[str, dict[str, Box]] = {}
    for label, fraction in (("part way", 0.5), ("all the way", 1.0)):
        pointer.glide(px + round(dx * fraction), py + round(dy * fraction), steps=12, ms=30)
        expect = want * fraction

        def moved(now: dict[str, Box], n: str) -> int:
            return edges(now[n])[OPPOSITE[held]] - edges(before[n])[OPPOSITE[held]]

        # the pointer is held there; the neighbors follow
        kde.wait_for(
            lambda expect=expect: all(abs(moved(frames(), n) - expect) <= CELL for n in near),
            timeout=5,
        )
        now = frames()
        held_at[label] = now
        print(f"{case} {label}: the pointer is at {where()}, the frames {now}")

        for n in near:
            if abs(moved(now, n) - expect) > CELL:
                problems.append(
                    f"{label}: {n}'s {OPPOSITE[held]} edge is {moved(now, n)} from where it was,"
                    f" the pointer {expect:.0f}"
                )
        for other, r in before.items():
            if other == name or other in near:
                continue
            if any(abs(edges(now[other])[e] - edges(r)[e]) > CELL for e in "LTRB"):
                # a tile that shares the dragged edge's line may move with it; nothing else may
                shared = any(abs(edges(r)[e] - edges(before[name])[held]) <= 80 for e in "LTRB")
                if not shared:
                    problems.append(f"{label}: {other} moved, and it is not next to the edge")

    pointer.up()
    kde.wait_for(lambda: resize_ends() > ends, timeout=10)
    after = settled()

    for n in near:
        jump = edges(after[n])[OPPOSITE[held]] - edges(held_at["all the way"][n])[OPPOSITE[held]]
        if abs(jump) > CELL:
            problems.append(f"{n} jumped {jump} when the button was let go")

    names = list(after)
    for i, n1 in enumerate(names):
        x, y, w, h = after[n1]
        if x < 0 or y < 0 or x + w > W or y + h > H:
            problems.append(f"{n1} {after[n1]} is off the screen")
        for n2 in names[i + 1 :]:
            ox, oy, ow, oh = after[n2]
            if x < ox + ow and ox < x + w and y < oy + oh and oy < y + h:
                problems.append(f"{n1} and {n2} overlap")

    print(f"{case}: neighbors {near}, before {before}, let go {after}")
    return problems


# BSP: win1 is the left half, win2 the top right and win3 the bottom right
cases = [
    "win2:left:-120,0",
    "win2:left:90,0",
    "win2:bottom:0,100",
    "win3:top:0,-80",
    "win1:right:100,0",
]
for case in cases:
    problems = run_case(case)
    checks.check(f"{case}: " + ("as expected" if not problems else "; ".join(problems)), not problems)

checks.finish()
