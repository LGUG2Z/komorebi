# Monitors that change, against a real headless KWin with two outputs. What only a real KWin can
# tell: that `kscreen-doctor` changing the scale of an output reaches komorebi as a change of that
# monitor's size, and the windows on it are tiled in the size it has now; that an output that is
# disabled (KWin's way to unplug one) takes its windows to the other output, where komorebi tiles
# them; and that the output enabled again is a monitor again, which windows can be sent to.
# `komorebi/tests/monitor_resize.rs` and `komorebi/tests/hotplug.rs` check the same against the
# fake.
from komorebi_vm import Checks, Kde, Rect

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

print("== 3. two windows go to the right monitor")
kde.komorebic("move-to-monitor", "1")
kde.komorebic("focus-monitor", "0")
kde.komorebic("move-to-monitor", "1")
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

print("== 4. the right output is unplugged")
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

print("== 5. it is plugged back")
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

print("== 6. and a window can be sent over again")
kde.komorebic("focus-monitor", "0")
kde.komorebic("move-to-monitor", "1")
checks.check(
    "komorebi has the window on the right monitor",
    kde.wait_for(lambda: containers_on(1) >= 1),
)
checks.check(
    "it is tiled on the right output",
    kde.wait_for(lambda: len(on_the_right()) >= 1 and tiled_in(on_the_right(), monitor(1))),
)

print("== 7. nothing is left behind")
checks.check(
    "komorebi is running and answers",
    kde.komorebi_running() and kde.run("komorebic state")[0] == 0,
)
kde.stop_komorebi()
checks.check("komorebi stopped", not kde.komorebi_running())

checks.finish()
