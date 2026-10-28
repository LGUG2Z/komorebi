# Monitors that change, against a real headless KWin with two outputs. What only a real KWin can
# tell: that `kscreen-doctor` changing the scale of an output reaches komorebi as a change of that
# monitor's size, that the windows on it are tiled again in the size it has now, and that they
# fill it again when it has its size back. `komorebi/tests/monitor_resize.rs` checks the same
# against the fake.
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


def tiled_in(area: Rect) -> bool:
    """Every one of the three windows is inside `area`, and none is on another.

    A pixel is allowed at each edge: at a fractional scale KWin reports frames that are not
    whole pixels.
    """
    rects = [kde.rect(kde.wid(title)) for title in TITLES]
    return all(r.inside(area, slack=1) for r in rects) and not any(
        a.overlaps(b) for i, a in enumerate(rects) for b in rects[i + 1 :]
    )


def resizes_handled() -> int:
    """How many times the daemon has read the sizes of its monitors again."""
    return kde.komorebi_log().count("updated monitor size and work area")


print("== 1. two outputs, and three windows on the left one")
left, right = kde.outputs.names()
print(f"     outputs: {left} (left), {right} (right)")
kde.start_komorebi()
checks.check(
    "the daemon answers komorebic, with both monitors",
    kde.wait_for(
        lambda: kde.run("komorebic state")[0] == 0
        and len(kde.state()["monitors"]["elements"]) == 2,
        timeout=30,
    ),
)
for title in TITLES:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
checks.check(
    "komorebi has the three windows on the left monitor",
    kde.wait_for(lambda: kde.managed(0) == 3),
)
checks.check("the left monitor has the size of its output", monitor(0) == Rect(0, 0, W, H))
checks.check("the windows are tiled inside it", kde.wait_for(lambda: tiled_in(monitor(0))))

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
    kde.wait_for(lambda: tiled_in(smaller)),
)

print("== 3. and back")
kde.outputs.scale(left, 1)
checks.check(
    "the monitor has its size back",
    kde.wait_for(lambda: monitor(0) == Rect(0, 0, W, H), timeout=20),
)
checks.check(
    "the windows fill it again, side by side",
    kde.wait_for(lambda: tiled_in(Rect(0, 0, W, H)) and kde.rect(kde.wid("w1")).w < W - 100),
)

print("== 4. nothing is left behind")
checks.check(
    "komorebi is running and answers",
    kde.komorebi_running() and kde.run("komorebic state")[0] == 0,
)
kde.stop_komorebi()
checks.check("komorebi stopped", not kde.komorebi_running())

checks.finish()
