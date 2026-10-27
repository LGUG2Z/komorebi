# KWin restarts under a running daemon, as when the compositor of a session crashes or is replaced:
# the session bus stays, KWin leaves it and comes back, and the clients of the old KWin go with it.
# What only a real KWin can tell: that the daemon sees KWin go and come back on the bus, that it
# takes every window of the old KWin out of its layout, that it loads its script into the new
# KWin, and that it tiles what opens there, right away. Twice, as the spike did.
from komorebi_vm import Checks, Kde, daemon

W, H = 1920, 1080
# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)


def daemon_log() -> str:
    return kde.komorebi_log()


def loads() -> int:
    """How many times the daemon has loaded its script into a KWin."""
    return daemon_log().count("loaded the KWin bridge script")


def where_asked(title: str) -> bool:
    """KWin has the window where the daemon asked: the same place, and the size up to a cell."""
    want, wid = daemon.asked(daemon_log(), title), kde.wid(title)
    if want is None or wid == "":
        return False
    have = kde.rect(wid)
    return (
        (have.x, have.y) == (want.x, want.y)
        and 0 <= want.w - have.w < CELL
        and 0 <= want.h - have.h < CELL
    )


def open_window(title: str) -> None:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda: kde.wid(title) != "", timeout=30)
    kde.wait_for(lambda: title in daemon.window_ids(daemon_log()), timeout=10)


def destroyed(titles: list[str]) -> bool:
    """The daemon took every one of these windows out of its layout."""
    log = daemon_log()
    ids = daemon.window_ids(log)
    gone = {e.window for e in daemon.events(log) if e.kind == "Destroy"}
    return all(ids.get(title) in gone for title in titles)


def alone(title: str) -> bool:
    """The window is where the daemon asked, and the only one: it has the whole width."""
    return where_asked(title) and kde.rect(kde.wid(title)).w > W - 100


def side_by_side(a: str, b: str) -> bool:
    if not (where_asked(a) and where_asked(b)):
        return False
    ra, rb = kde.rect(kde.wid(a)), kde.rect(kde.wid(b))
    return not ra.overlaps(rb) and ra.w < W - 20 and rb.w < W - 20


def restart(old: list[str], loaded: int) -> None:
    kde.restart_kwin()
    checks.check(
        "the daemon saw KWin go",
        kde.wait_for(lambda: "KWin went away" in daemon_log(), timeout=10),
    )
    checks.check(
        "every window of the old KWin left the layout", kde.wait_for(lambda: destroyed(old))
    )
    checks.check(
        "the daemon loaded its script into the new KWin",
        kde.wait_for(lambda: loads() == loaded, timeout=30),
    )
    checks.check("the new KWin runs the script", kde.wait_for(kde.script_loaded))
    checks.check("the daemon is still running", kde.komorebi_running())


print("== 1. three windows are tiled")
kde.start_komorebi()
checks.check(
    "the daemon loaded its script into KWin", kde.wait_for(lambda: loads() == 1, timeout=30)
)
before = ["pre1", "pre2", "pre3"]
for title in before:
    open_window(title)
checks.check(
    "every window is where the daemon asked for it",
    kde.wait_for(lambda: all(where_asked(t) for t in before), timeout=20),
)

print("== 2. KWin restarts")
restart(before, loaded=2)
checks.check("the windows of the old KWin are gone", all(kde.wid(t) == "" for t in before))

print("== 3. windows open in the new KWin")
open_window("post1")
checks.check("the first is tiled on its own", kde.wait_for(lambda: alone("post1"), timeout=20))
open_window("post2")
checks.check(
    "the second is tiled next to it",
    kde.wait_for(lambda: side_by_side("post1", "post2"), timeout=20),
)
for title in ["post1", "post2"]:
    asked = daemon.asked(daemon_log(), title)
    print(f"     {title}: asked {asked}, has {kde.rect(kde.wid(title))}")

print("== 4. KWin restarts again, with one window open")
kde.close(kde.wid("post2"))
checks.check(
    "the window that is left has the screen", kde.wait_for(lambda: alone("post1"), timeout=20)
)
restart(["post1"], loaded=3)
open_window("again")
checks.check(
    "a window opened in the third KWin is tiled", kde.wait_for(lambda: alone("again"), timeout=20)
)

print("== 5. stop")
kde.stop_komorebi()
checks.check("the daemon exited", not kde.komorebi_running())
checks.check("the script is no longer in KWin", kde.wait_for(lambda: not kde.script_loaded()))

kde.stop()
checks.finish()
