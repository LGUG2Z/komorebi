# Stacking on a real KWin: what komorebi believes (`komorebic state`) against what KWin actually
# has (`kdotool`). A stack shows one window, and the rest of it is out of sight on the parking
# desktop, a virtual desktop nobody looks at; the window that is shown is the one that has the
# focus. KWin keeps its virtual desktops in `kwinrc` as soon as one is made, so what only a real
# KWin can tell is that the parking desktop is made when it is needed, that it is removed when
# komorebi stops, and that one a crashed komorebi left behind is swept by the next. The windows a
# stack hides are on a desktop of their own, not minimized: their events are not the user's.
# `komorebi/tests/stacks.rs` checks which windows a stack holds against the fake.
from komorebi_vm import Checks, Kde, Rect

W, H = 1920, 1080
PARK = "komorebi-park"
TITLES = ["win1", "win2", "win3"]

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)


def sizes() -> list[int]:
    """How many windows each container of the first workspace has, in order."""
    return [len(container) for container in kde.containers()]


def park_desktop() -> int:
    """The number of the parking desktop, counted from 1 as kdotool counts, or 0 for none."""
    names = kde.desktop_names()
    return names.index(PARK) + 1 if PARK in names else 0


def parked() -> list[str]:
    """The titles of the windows that are on the parking desktop."""
    park = park_desktop()
    if park == 0:
        return []
    found = [t for t in TITLES if kde.wid(t) != "" and kde.desk(kde.wid(t)) == park]
    return sorted(found)


def active_title() -> str:
    active = kde.active()
    return kde.title(active) if active else ""


def agree() -> bool:
    """KWin and komorebi have the same window focused."""
    return active_title() != "" and active_title() == kde.focused_title()


def inside(r: Rect) -> bool:
    return r.x >= 0 and r.y >= 0 and r.x + r.w <= W and r.y + r.h <= H


def on_a_workspace_desktop() -> bool:
    """Every window that is left is on a desktop that is a workspace, not on the parking one."""
    return all(kde.desk(kde.wid(t)) in (1, 2) for t in TITLES if kde.wid(t) != "")


print("== 1. three windows")
kde.start_komorebi()
kde.wait_for(lambda: kde.run("komorebic monitor-information")[0] == 0, timeout=30)
for title in TITLES:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
checks.check("komorebi manages the three", kde.wait_for(lambda: kde.managed() == 3, timeout=20))
checks.check("there are three tiles", sizes() == [1, 1, 1])
checks.check("nothing is out of sight, and there is no parking desktop yet", park_desktop() == 0)

print("== 2. stack the focused window onto its neighbour")
kde.komorebic("stack", "left")
checks.check(
    "two containers, one of them a stack of two",
    kde.wait_for(lambda: sorted(sizes()) == [1, 2]),
)
print(f"     containers: {kde.containers()}, out of sight: {parked()}")
checks.check(
    "there is a parking desktop, and exactly one window is on it",
    kde.wait_for(lambda: park_desktop() != 0 and len(parked()) == 1),
)
checks.check("the window in view is the one with the focus", kde.wait_for(agree))
shown = active_title()
checks.check("and the one out of sight is not it", parked() != [shown])
checks.check(
    "the parking desktop is the last, so it is not a workspace",
    park_desktop() == kde.desktops(),
)
checks.check(
    "komorebi still has all three windows on workspace 0, the hidden one among them",
    kde.managed(0) == 3,
)
rect_shown = kde.rect(kde.wid(shown))
checks.check("the one in view is inside the screen", inside(rect_shown))

print("== 3. a stack is still a stack after a workspace round trip")
before = parked()
kde.komorebic("focus-workspace", "1")
checks.check("workspace 1 is showing", kde.wait_for(lambda: kde.shown() == 2))
kde.komorebic("focus-workspace", "0")
checks.check("workspace 0 is showing again", kde.wait_for(lambda: kde.shown() == 1))
checks.check(
    f"the same window is out of sight ({before})",
    kde.wait_for(lambda: parked() == before),
)
checks.check("and the focus agrees", kde.wait_for(agree))

print("== 4. unstack")
kde.komorebic("unstack")
checks.check("three tiles again", kde.wait_for(lambda: sizes() == [1, 1, 1]))
checks.check("nothing is out of sight", kde.wait_for(lambda: parked() == []))
rects = [kde.rect(kde.wid(t)) for t in TITLES]
checks.check("they are inside the screen", all(inside(r) for r in rects))
checks.check(
    "and they tile, not on each other",
    not any(a.overlaps(b) for i, a in enumerate(rects) for b in rects[i + 1 :]),
)

print("== 5. closing the window in view brings the next one")
kde.komorebic("stack", "left")
checks.check("stacked again", kde.wait_for(lambda: len(parked()) == 1))
hidden_one = parked()[0]
kde.komorebic("close")
checks.check(
    "the one that was out of sight is in view and has the focus",
    kde.wait_for(lambda: parked() == [] and active_title() == hidden_one and agree()),
)
checks.check("two tiles are left", kde.wait_for(lambda: sizes() == [1, 1]))

print("== 6. stack all, and unstack all")
kde.komorebic("stack-all")
checks.check(
    "every window is in one stack, with one in view",
    kde.wait_for(lambda: sizes() == [2] and len(parked()) == 1),
)
checks.check("the window in view has the focus", kde.wait_for(agree))
kde.komorebic("unstack-all")
checks.check(
    "they have a tile each again, and none is out of sight",
    kde.wait_for(lambda: sizes() == [1, 1] and parked() == []),
)

print("== 7. a crash leaves a parking desktop, which the next start sweeps")
kde.komorebic("stack-all")
checks.check("a window is parked", kde.wait_for(lambda: len(parked()) == 1))
names_before = kde.desktop_names()
kde.kill_komorebi()
checks.check(
    "the daemon is dead and the parking desktop is still there, with a window on it",
    PARK in kde.desktop_names() and len(parked()) == 1,
)
kde.start_komorebi()
checks.check(
    "the next start removed the stale parking desktop",
    kde.wait_for(lambda: kde.run("komorebic monitor-information")[0] == 0 and park_desktop() == 0),
)
checks.check("and brought the parked window back", kde.wait_for(on_a_workspace_desktop))
checks.check(
    "the workspaces' desktops are left alone",
    kde.desktop_names() == [n for n in names_before if n != PARK],
)
checks.check("komorebi manages the two windows again", kde.wait_for(lambda: kde.managed() == 2))
kde.komorebic("stack-all")
checks.check(
    "this run makes a parking desktop of its own and hides the back of the stack on it",
    kde.wait_for(lambda: park_desktop() != 0 and len(parked()) == 1),
)

print("== 8. a clean stop puts every window back")
still_open = [t for t in TITLES if kde.wid(t) != ""]
kde.komorebic("stop")
checks.check("komorebi exited", kde.wait_for(lambda: not kde.komorebi_running()))
checks.check("the parking desktop is gone", kde.wait_for(lambda: park_desktop() == 0))
checks.check(
    "and every window is on a workspace's desktop",
    kde.wait_for(on_a_workspace_desktop),
)
checks.check("the windows are still open", all(kde.wid(t) != "" for t in still_open))

kde.stop()
checks.finish()
