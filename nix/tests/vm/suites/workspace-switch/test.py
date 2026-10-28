# komorebi's workspaces are KWin's own virtual desktops, per output. What only a real KWin can
# tell: that komorebi turns on KWin's per-output virtual desktops and puts the user's setting back
# when it stops; that `komorebic send-to-workspace` puts a window on another desktop and leaves the
# one in front showing; that `komorebic focus-workspace` has KWin show the other desktop, and KWin
# focuses the window there, which komorebi agrees with, without komorebi hiding or parking any
# window; that a switch by anything else (a shortcut, an app raising a window of another desktop)
# is followed once, with KWin and komorebi agreeing on the focused window and nothing flip-flopping;
# and that a window moved to another desktop by someone else (the window menu's "Move to Desktop")
# moves to that workspace. `komorebi/tests/workspaces.rs` checks the same against the fake.
import time

from komorebi_vm import Checks, Kde, daemon

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)


def focus_changes() -> int:
    return sum(1 for e in daemon.events(kde.komorebi_log()) if e.kind == "FocusChange")


def ws() -> int:
    """The workspace komorebi has in front on the (only) monitor."""
    return kde.state()["monitors"]["elements"][0]["workspaces"]["focused"]


def active_title() -> str:
    active = kde.active()
    return kde.title(active) if active else ""


def per_output() -> bool:
    return kde.kwinrc("Windows", "PerOutputVirtualDesktops") == "true"


print("== 1. start, and three windows")
checks.check("per-output virtual desktops are off before komorebi starts", not per_output())
kde.start_komorebi()
checks.check(
    "the daemon answers komorebic",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=30),
)
for title in ["w1", "w2", "w3"]:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
checks.check("komorebi manages the three", kde.wait_for(lambda: kde.managed(0) == 3, timeout=20))

print("== 2. workspaces are desktops")
checks.check("komorebi turned on per-output virtual desktops in kwinrc", per_output())
checks.check(
    "all three windows are on desktop 1, which is showing",
    all(kde.desk(kde.wid(t)) == 1 for t in ["w1", "w2", "w3"]) and kde.shown() == 1,
)

print("== 3. one window goes to workspace 1 and stays there")
moved = active_title()
mid = kde.wid(moved)
kde.komorebic("send-to-workspace", "1")
checks.check(
    f"{moved} is on desktop 2, and desktop 1 is still showing",
    kde.wait_for(lambda: kde.desk(mid) == 2 and kde.shown() == 1 and ws() == 0),
)
checks.check(
    "the other two are on workspace 0, and komorebi has the moved one on workspace 1",
    kde.wait_for(lambda: kde.managed(0) == 2 and kde.managed(1) == 1),
)
stayed = [t for t in ["w1", "w2", "w3"] if t != moved]

print("== 4. komorebic switches workspace: KWin shows the desktop, komorebi hides nothing")
kde.komorebic("focus-workspace", "1")
checks.check(
    "desktop 2 is showing and komorebi agrees",
    kde.wait_for(lambda: kde.shown() == 2 and ws() == 1),
)
checks.check(
    "KWin focused the window of that desktop and komorebi knows it",
    kde.wait_for(lambda: active_title() == moved and kde.focused_title(1) == moved),
)
checks.check(
    "the windows of desktop 1 never left it", all(kde.desk(kde.wid(t)) == 1 for t in stayed)
)
checks.check("komorebi made one desktop, for workspace 1, and no other", kde.desktops() == 2)
kde.komorebic("focus-workspace", "0")
checks.check("back on desktop 1", kde.wait_for(lambda: kde.shown() == 1 and ws() == 0))

print("== 5. something else switches: a pager or a shortcut")
kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin setCurrentDesktop i 2 > /dev/null")
checks.check("komorebi followed to workspace 1", kde.wait_for(lambda: ws() == 1))
checks.check(
    "and agrees with KWin on the focused window",
    kde.wait_for(lambda: active_title() != "" and active_title() == kde.focused_title(1)),
)

print("== 6. an app raises a window of another desktop")
kde.sh("busctl --user call org.kde.KWin /KWin org.kde.KWin setCurrentDesktop i 1 > /dev/null")
kde.wait_for(lambda: ws() == 0)
time.sleep(1 * SCALE)
before = focus_changes()
kde.activate(mid)
checks.check(
    "KWin switched to the window's desktop and komorebi followed",
    kde.wait_for(lambda: kde.shown() == 2 and ws() == 1),
)
checks.check(
    f"KWin and komorebi agree on the focused window: {moved}",
    kde.wait_for(lambda: active_title() == moved and kde.focused_title(1) == moved),
)
time.sleep(3 * SCALE)
after = focus_changes()
print(f"     focus changes komorebi handled meanwhile: {after - before}")
checks.check(
    f"nothing is flip-flopping (at most 6 focus events, got {after - before})",
    after - before <= 6,
)
time.sleep(3 * SCALE)
final = focus_changes()
checks.check(f"and it stays quiet afterwards ({final - after} more)", final - after <= 1)

print("== 7. somebody moves a window to another desktop (the window menu's Move to Desktop)")
other = kde.wid(stayed[0])
kde.sh(f"kdotool set_desktop_for_window {other} 2")
checks.check(
    "komorebi moved it to workspace 1: one window left on workspace 0, two on workspace 1",
    kde.wait_for(lambda: kde.managed(0) == 1 and kde.managed(1) == 2),
)
checks.check("and left it where KWin put it", kde.desk(other) == 2)
checks.check("KWin carried out every command", "the bridge could not do" not in kde.komorebi_log())

print("== 8. a clean stop")
kde.komorebic("stop")
checks.check("the daemon exited", kde.wait_for(lambda: not kde.komorebi_running()))
checks.check("per-output virtual desktops are back as they were (off)", not per_output())
checks.check("the desktops komorebi made stay: they are the user's now", kde.desktops() == 2)

kde.stop()
checks.finish()
