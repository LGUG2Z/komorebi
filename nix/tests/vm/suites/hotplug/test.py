# Monitors that change, against a real headless KWin with two outputs. What only a real KWin can
# tell: that `kscreen-doctor` changing the scale of an output reaches komorebi as a change of that
# monitor's size, and the windows on it are tiled in the size it has now; that a window moved to
# the other output by something other than komorebi (what a shortcut that sends a window to the
# next screen does) is tiled on that monitor, and one komorebi moved itself is left where it is;
# that an output that is disabled (KWin's way to unplug one) takes its windows to the other
# output, where komorebi tiles them; and that the output enabled again is a monitor again, which
# windows can be sent to. `komorebi/tests/monitor_resize.rs`, `komorebi/tests/hotplug.rs` and
# `komorebi/tests/follow_output.rs` check the same against the fake.
import time

from komorebi_vm import Checks, Kde, Rect, daemon
from komorebi_vm.core.platform import Client
from komorebi_vm.core.scenarios import hotplug
from komorebi_vm.kde_platform import KdePlatform

W, H = 1920, 1080

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), outputs=2, scale=SCALE)

TITLES = ["w1", "w2", "w3"]


def monitor(idx: int) -> Rect:
    """The bounds komorebi has for a monitor."""
    size = kde.state()["monitors"]["elements"][idx]["size"]
    return Rect(size["left"], size["top"], size["right"], size["bottom"])


def monitors() -> int:
    """How many monitors komorebi has."""
    return len(kde.state()["monitors"]["elements"])


def containers_on(idx: int) -> int:
    """How many containers the workspace in front of a monitor has in komorebi."""
    workspaces = kde.state()["monitors"]["elements"][idx]["workspaces"]
    return len(workspaces["elements"][workspaces["focused"]]["containers"]["elements"])


def rects(titles: list[str]) -> list[Rect]:
    """The frames KWin has for these windows."""
    return [kde.rect(kde.wid(title)) for title in titles]


def tiled_in(titles: list[str], area: Rect) -> bool:
    """Every one of these windows is inside `area`, and none is on another.

    A pixel is allowed at each edge: at a fractional scale KWin reports frames that are not
    whole pixels.
    """
    frames = rects(titles)
    return all(r.inside(area, slack=1) for r in frames) and not any(
        a.overlaps(b) for i, a in enumerate(frames) for b in frames[i + 1 :]
    )


def on_the_right() -> list[str]:
    """The windows that are on the right output, as KWin has them."""
    return [t for t, r in zip(TITLES, rects(TITLES), strict=True) if r.x >= W]


def output_changes() -> int:
    """How many times KWin has told the daemon that a window is on another output."""
    return sum(1 for e in daemon.events(kde.komorebi_log()) if e.kind == "OutputChange")


def output_lines() -> list[str]:
    """The lines of the daemon's log about windows on other outputs, for a failure to be read."""
    log = kde.komorebi_log().splitlines()
    return [line[-160:] for line in log if "OutputChange" in line or "following them" in line][-8:]


def let_komorebi_finish() -> None:
    """Waits until the bridge has stopped asking again for the position of the windows komorebi
    just moved.

    The bridge asks again for a second after each command (`WANT_MS` in `bridge.js`), which puts
    back a window that somebody moves in that time. A user moves a window later than that, so a
    test that plays the user does too.
    """
    time.sleep(2 * SCALE)


def show(what: str) -> None:
    """Prints where komorebi has the windows and where KWin has them, for a failure to be read."""
    state = f"{containers_on(0)} on the left monitor, {containers_on(1)} on the right"
    frames = ", ".join(f"{t} {r}" for t, r in zip(TITLES, rects(TITLES), strict=True))
    print(f"     {what}: komorebi has {state}; KWin has {frames}")


def resizes_handled() -> int:
    """How many times the daemon has read the sizes of its monitors again."""
    return kde.komorebi_log().count("updated monitor size and work area")


print("== 1. two outputs, and three windows on the left one")
left, right = kde.outputs.names()
print(f"     outputs: {left} (left), {right} (right)")
kde.start_komorebi()
checks.check(
    "the daemon answers komorebic, with both monitors",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0 and monitors() == 2, timeout=30),
)
for title in TITLES:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
checks.check(
    "komorebi has the three windows on the left monitor",
    kde.wait_for(lambda: containers_on(0) == 3 and containers_on(1) == 0),
)
checks.check("the left monitor has the size of its output", monitor(0) == Rect(0, 0, W, H))
checks.check("the windows are tiled inside it", kde.wait_for(lambda: tiled_in(TITLES, monitor(0))))

print("== 2. the scale of the left output changes")
before = resizes_handled()
kde.outputs.scale(left, 1.5)
smaller = Rect(0, 0, W * 2 // 3, H * 2 // 3)
checks.check(
    f"komorebi has the new size of the monitor, {smaller}",
    kde.wait_for(lambda: monitor(0) == smaller, timeout=20),
)
checks.check("it read the sizes again for it", resizes_handled() > before)
checks.check(
    "the windows are tiled again inside the monitor as it is now",
    kde.wait_for(lambda: tiled_in(TITLES, smaller)),
)
kde.outputs.scale(left, 1)
checks.check(
    "the monitor has its size back, and the windows fill it again",
    kde.wait_for(lambda: monitor(0) == Rect(0, 0, W, H) and tiled_in(TITLES, monitor(0)), 20),
)

print("== 3. komorebi sends a window to the right monitor itself")
changes = output_changes()
kde.komorebic("move-to-monitor", "1")
checks.check(
    "komorebi has two containers on the left monitor and one on the right",
    kde.wait_for(lambda: containers_on(0) == 2 and containers_on(1) == 1),
)
checks.check(
    "KWin reported the window on the other output",
    kde.wait_for(lambda: output_changes() > changes),
)
moved = on_the_right()
checks.check(
    "the window is tiled on the right output",
    len(moved) == 1 and tiled_in(moved, monitor(1)),
)
checks.check(
    "and it is still on the right monitor once KWin has said so",
    containers_on(0) == 2 and containers_on(1) == 1,
)

print("== 4. the user sends another window to the right screen")
sent = [t for t in TITLES if t not in moved][0]
let_komorebi_finish()
kde.sh(f"kdotool windowmove {kde.wid(sent)} 2100 300")
checks.check(
    "komorebi has one container on the left monitor and two on the right",
    kde.wait_for(lambda: containers_on(0) == 1 and containers_on(1) == 2),
)
checks.check(
    "the two are tiled on the right output, and the third fills the left one",
    kde.wait_for(
        lambda: len(on_the_right()) == 2
        and tiled_in(on_the_right(), monitor(1))
        and tiled_in([t for t in TITLES if t not in on_the_right()], monitor(0))
    ),
)
show("after the user's move")
print("     " + "\n     ".join(output_lines()))

print("== 5. the right output is unplugged")
kde.outputs.disable(right)
checks.check("komorebi has one monitor", kde.wait_for(lambda: monitors() == 1, timeout=30))
checks.check("and all three windows on it", kde.wait_for(lambda: containers_on(0) == 3))
checks.check(
    "they are tiled inside the screen that is left, and not on each other",
    kde.wait_for(lambda: tiled_in(TITLES, Rect(0, 0, W, H))),
)
checks.check(
    "komorebi is alive and its idea of the focus is KWin's",
    kde.wait_for(
        lambda: kde.komorebi_running()
        and kde.active() != ""
        and kde.title(kde.active()) == kde.focused_title(0)
    ),
)

print("== 6. it is plugged back")
kde.outputs.enable(right)
checks.check("komorebi has two monitors again", kde.wait_for(lambda: monitors() == 2, timeout=30))
checks.check("and still three windows", kde.wait_for(lambda: kde.managed_all() == 3))
checks.check(
    "each window is somewhere it can be seen",
    kde.wait_for(
        lambda: all(
            r.inside(Rect(0, 0, W, H), 1) or r.inside(Rect(W, 0, W, H), 1) for r in rects(TITLES)
        )
    ),
)

print("== 7. and a window can be sent over again")
let_komorebi_finish()
kde.sh(f"kdotool windowmove {kde.wid('w3')} 2100 300")
checks.check(
    "komorebi follows it: one container on the right monitor",
    kde.wait_for(lambda: containers_on(1) == 1),
)
checks.check(
    "it is tiled on the right output, and the others together on the left",
    kde.wait_for(
        lambda: on_the_right() == ["w3"]
        and tiled_in(["w3"], monitor(1))
        and tiled_in(["w1", "w2"], monitor(0))
    ),
)
show("after the second move")
print("     " + "\n     ".join(output_lines()))

print("== 7b. the same, through the neutral platform")
# the scenario of komorebi_vm.core.scenarios.hotplug is what runs on Windows too: it unplugs a
# display, plugs one in and sends a window to it, and asks the operating system where the windows are
hotplug.run(KdePlatform(kde, (W, H)), checks, [Client(t) for t in TITLES])

print("== 8. nothing is left behind")
checks.check(
    "komorebi is running and answers",
    kde.komorebi_running() and kde.run("komorebic state")[0] == 0,
)
kde.stop_komorebi()
checks.check("komorebi stopped", not kde.komorebi_running())

checks.finish()
