# The daemon reads a real KWin through its script. What only a real KWin can tell: that the script
# loads and reports the desktop, that KWin's signals reach the window manager as the events
# `komorebi/src/kwin/state.rs` and `komorebi/tests/wm_events.rs` feed it by hand, in the order KWin
# raised them (KWin activates a new window before it announces it), and that it takes its script
# out of KWin when it stops.
from komorebi_vm import Checks, Kde, daemon
from komorebi_vm.daemon import Event

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1280, 720), scale=SCALE)


def daemon_log() -> str:
    return kde.komorebi_log()


def handled(event: Event) -> bool:
    return event in daemon.events(daemon_log())


kde.spawn("foot -T before sleep 3000")
checks.check(
    "a window is open before komorebi starts",
    kde.wait_for(lambda: kde.wid("before") != "", timeout=30),
)
before = kde.wid("before")

kde.start_komorebi("--log-level", "debug")
checks.check(
    "the daemon loaded its script into KWin",
    kde.wait_for(lambda: "loaded the KWin bridge script" in daemon_log(), timeout=30),
)
checks.check("KWin runs the script", kde.script_loaded())
checks.check(
    "the script reported the window that was already open",
    kde.wait_for(lambda: "1 valid windows identified" in daemon_log(), timeout=10),
)
checks.check("the daemon is still running", kde.komorebi_running())

print("== a window opens")
kde.spawn("foot -T alpha sleep 3000")
checks.check("KWin has it", kde.wait_for(lambda: kde.wid("alpha") != "", timeout=30))
alpha = kde.wid("alpha")
checks.check(
    "KWin's focus on it reached the daemon, by title",
    kde.wait_for(lambda: "alpha" in daemon.window_ids(daemon_log()), timeout=10),
)
ids = daemon.window_ids(daemon_log())
n = ids.get("alpha", -1)
shown = Event("Show", n, "WindowAdded")
focused = Event("FocusChange", n, "WindowActivated")
checks.check("the daemon handled a Show for it", kde.wait_for(lambda: handled(shown), timeout=10))
checks.check("... and a FocusChange", kde.wait_for(lambda: handled(focused), timeout=10))
seen = daemon.events(daemon_log())
checks.check(
    "the Show came first, although KWin activates a window before it announces it",
    shown in seen and focused in seen and seen.index(shown) < seen.index(focused),
)

print("== the focus moves")
kde.activate(before)
checks.check(
    "the window that was open before is focused, and the daemon knows it",
    kde.wait_for(lambda: "before" in daemon.window_ids(daemon_log()), timeout=10),
)
m = daemon.window_ids(daemon_log()).get("before", -1)
checks.check(
    "a FocusChange for it",
    kde.wait_for(lambda: handled(Event("FocusChange", m, "WindowActivated")), timeout=10),
)

print("== minimize, restore, close")
kde.minimize(alpha)
checks.check(
    "minimizing is a Minimize",
    kde.wait_for(lambda: handled(Event("Minimize", n, "WindowMinimized")), timeout=10),
)
kde.activate(alpha)
checks.check(
    "activating a minimized window brings it back: a Restore",
    kde.wait_for(lambda: handled(Event("Restore", n, "WindowRestored")), timeout=10),
)
kde.close(alpha)
checks.check(
    "closing is a Destroy",
    kde.wait_for(lambda: handled(Event("Destroy", n, "WindowRemoved")), timeout=10),
)
mine = [e.kind for e in daemon.events(daemon_log()) if e.window == n]
print(f"     the events of alpha, in order: {mine}")
in_order = [kind for kind in mine if kind in ("Show", "Minimize", "Restore", "Destroy")]
checks.check(
    "alpha's events came in the order they happened",
    in_order == ["Show", "Minimize", "Restore", "Destroy"],
)

print("== stop")
kde.stop_komorebi()
checks.check("the daemon exited", not kde.komorebi_running())
checks.check(
    "the script is no longer in KWin", kde.wait_for(lambda: not kde.script_loaded(), timeout=10)
)
checks.check("the windows are still there", kde.wid("before") != "")

kde.stop()
checks.finish()
