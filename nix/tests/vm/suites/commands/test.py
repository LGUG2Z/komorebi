# Every layout, and a sweep of komorebic's commands, against a real headless KWin with two outputs.
#
# After each command the same things have to be true: komorebi is alive, it can still say what it
# knows (`komorebic state`), and every window it has is a window KWin has. After each layout, the
# windows are inside the outputs and do not lie on each other. A command is also expected to be
# understood (exit status 0). The Rust tests check what each command does against the fake; this
# suite checks that none of them breaks the daemon in a real session, and that what the daemon
# asks KWin for is what KWin carries out. Every command `komorebic` has is in the sweep.
import shlex

from komorebi_vm import Checks, Kde, daemon

W, H = 1920, 1080
OUTPUTS = 2
# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24
LAYOUTS = [
    "bsp",
    "columns",
    "rows",
    "vertical-stack",
    "horizontal-stack",
    "ultrawide-vertical-stack",
    "grid",
    "right-main-vertical-stack",
    "scrolling",
]

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), outputs=OUTPUTS, scale=SCALE)


def live_windows() -> list[str]:
    """The ids of the windows KWin has: every client of this suite is a foot."""
    return kde.run("kdotool search --class foot")[1].split()


def healthy() -> int:
    """0 when komorebi is running, answers `komorebic state` and has every window KWin has.

    Otherwise the reason: 1 not running, 2 no state, 3 not the windows KWin has.
    """
    if not kde.komorebi_running():
        return 1
    status, _ = kde.run("komorebic state")
    if status != 0:
        return 2
    if kde.managed_all() != len(live_windows()):
        return 3
    return 0


def outside_or_overlapping() -> list[str]:
    """The windows that are outside the outputs, or on another window (2 pixels are allowed)."""
    rects = {wid: kde.rect(wid) for wid in live_windows()}
    bad = []
    for wid, r in rects.items():
        if r.x < -1 or r.y < -1 or r.x + r.w > W * OUTPUTS + 1 or r.y + r.h > H + 1:
            bad.append(f"{kde.title(wid)} outside at {r}")
    ids = list(rects)
    for i, a in enumerate(ids):
        for b in ids[i + 1 :]:
            ra, rb = rects[a], rects[b]
            if (
                ra.x < rb.x + rb.w - 2
                and rb.x < ra.x + ra.w - 2
                and ra.y < rb.y + rb.h - 2
                and rb.y < ra.y + ra.h - 2
            ):
                bad.append(f"{kde.title(a)} on {kde.title(b)}")
    return bad


def open_window(n: int) -> None:
    title = f"win{n}"
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda: kde.wid(title) != "", timeout=30)


def close_window(n: int) -> None:
    title = f"win{n}"
    kde.close(kde.wid(title))
    kde.wait_for(lambda: kde.wid(title) == "", timeout=10)


def where_asked(wid: str) -> str | None:
    """None when KWin has the window where the daemon last asked, else what is wrong."""
    title = kde.title(wid)
    want, have = daemon.asked(kde.komorebi_log(), title), kde.rect(wid)
    if want is None:
        return f"{title}: the daemon never asked for a frame"
    if (have.x, have.y) != (want.x, want.y) or not (
        0 <= want.w - have.w < CELL and 0 <= want.h - have.h < CELL
    ):
        return f"{title}: asked {want}, has {have}"
    return None


print("== 1. start, and five windows on the first output")
kde.start_komorebi()
checks.check(
    "the daemon answers komorebic, with both outputs",
    kde.wait_for(
        lambda: kde.run("komorebic monitor-information")[0] == 0
        and len(kde.state()["monitors"]["elements"]) == OUTPUTS,
        timeout=30,
    ),
)
for n in range(1, 6):
    open_window(n)
checks.check(
    "five windows, and komorebi has all five",
    kde.wait_for(lambda: kde.managed() == 5 and healthy() == 0, timeout=20),
)

print("== 2. every layout, with five, three, two and one windows")
for layout in LAYOUTS:
    kde.komorebic("change-layout", layout)
    verdict = []
    for keep in (5, 3, 2, 1):
        while kde.managed() > keep:
            close_window(kde.managed())
            kde.wait_for(lambda: healthy() == 0, timeout=10)
        # the daemon retiles on the close; give KWin the time to carry it out
        kde.wait_for(lambda: all(where_asked(w) is None for w in live_windows()), timeout=10)
        # the scrolling layout is a strip whose columns can be past the edge of the output
        if layout != "scrolling":
            verdict += [f"{keep}w: {bad}" for bad in outside_or_overlapping()]
        reason = healthy()
        if reason:
            verdict.append(f"{keep}w: not healthy ({reason})")
    checks.check(
        f"{layout}: the windows are in their outputs and do not overlap, and komorebi is healthy"
        + "".join(f"\n       {v}" for v in verdict),
        verdict == [],
    )
    for n in range(kde.managed() + 1, 6):
        open_window(n)
    kde.wait_for(lambda: kde.managed() == 5, timeout=20)
kde.komorebic("change-layout", "bsp")

print("== 3. a sweep of commands (each must be understood, and leave komorebi healthy)")
SWEEP = """
focus left
focus right
focus up
focus down
move left
move right
move up
move down
cycle-focus next
cycle-focus previous
cycle-move next
cycle-move previous
retile
flip-layout horizontal
flip-layout vertical
flip-layout horizontal-and-vertical
flip-layout horizontal-and-vertical
cycle-layout next
cycle-layout previous
change-layout columns
change-layout bsp
preselect-direction left
cancel-preselect
toggle-tiling
toggle-tiling
mouse-follows-focus enable
mouse-follows-focus disable
toggle-mouse-follows-focus
toggle-mouse-follows-focus
retile
"""
for line in SWEEP.strip().splitlines():
    status, out = kde.run(shlex.join(["komorebic", *line.split()]))
    if status != 0:
        checks.bad(f"komorebic {line}: exit {status}: {' '.join(out.splitlines()[:2])}")
        continue
    if not kde.wait_for(lambda: healthy() == 0, timeout=5):
        checks.bad(f"komorebic {line}: komorebi is not healthy afterwards ({healthy()})")
        continue
    checks.ok(f"komorebic {line}")

print("== 4. what komorebic only prints")
for args in ["state", "monitor-information", "data-directory"]:
    checks.check(f"komorebic {args}", kde.run(f"komorebic {args}")[0] == 0)
for query in [
    "focused-monitor-index",
    "focused-workspace-index",
    "focused-container-index",
    "focused-window-index",
    "focused-workspace-layout",
    "focused-container-kind",
    "version",
]:
    status, out = kde.run(f"komorebic query {query}")
    checks.check(f"query {query} answers", status == 0 and out.strip() != "")

print("== 5. still all there")
checks.check("komorebi has every window KWin has, and answers", healthy() == 0)
for wid in live_windows():
    print(f"     {kde.title(wid)}: {kde.rect(wid)}")
mismatch = [m for m in (where_asked(w) for w in live_windows()) if m is not None]
for m in mismatch:
    print(f"     {m}")
checks.check("every window is where komorebi last asked for it", mismatch == [])
kde.komorebic("stop")
checks.check("a clean stop", kde.wait_for(lambda: not kde.komorebi_running()))

kde.stop()
checks.finish()
