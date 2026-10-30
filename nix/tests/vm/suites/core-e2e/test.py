# komorebi tiles a real KWin. What only a real KWin can tell: that the commands the daemon queues
# reach KWin through the held call and that KWin carries them out, so that each window is where
# the daemon asked for it (`komorebi/tests/wm_events.rs` checks the window manager's side against
# the fake), that the daemon and KWin agree on which window has the focus, that `komorebic` reaches
# the daemon of a session and reads its state, that `komorebic focus` and `cycle-focus` move KWin's
# focus (`komorebi/tests/focus.rs` checks which window they pick), that `komorebic move` and
# `cycle-move` swap windows between their tiles (`komorebi/tests/moving.rs`), that
# `komorebic toggle-monocle` gives one window the whole work area and puts it back, and hides the
# others on the parking desktop, which a clean stop and the next start after a crash put right
# (`komorebi/tests/monocle.rs`), and that `komorebic stop` gives KWin back what the daemon set up in
# it. A window manager that agrees with itself proves nothing, so what the daemon believes is read
# from its debug log and from `komorebic state`, and what KWin has through kdotool.
import json

from komorebi_vm import Checks, Kde, Rect, daemon
from komorebi_vm.core.platform import Client
from komorebi_vm.core.scenarios import (
    closing,
    commands,
    flipping,
    floating,
    focus,
    layouts,
    lifecycle,
    maximizing,
    minimizing,
    monocle,
    moving,
    padding,
    placement,
    promoting,
    resize_commands,
    rules,
    stacks,
    startup_rules,
    tiling,
    workspace_moves,
    workspaces,
)
from komorebi_vm.kde_platform import KdePlatform

W, H = 1920, 1080
PARK = "komorebi-park"
# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)


def daemon_log() -> str:
    return kde.komorebi_log()


def inside(r: Rect) -> bool:
    return r.x >= 0 and r.y >= 0 and r.x + r.w <= W and r.y + r.h <= H


def asked(title: str) -> Rect | None:
    return daemon.asked(daemon_log(), title)


def where_asked(title: str) -> bool:
    """KWin has the window where the daemon asked: the same place, and the size up to a cell."""
    want, wid = asked(title), kde.wid(title)
    if want is None or wid == "":
        return False
    have = kde.rect(wid)
    return (
        (have.x, have.y) == (want.x, want.y)
        and 0 <= want.w - have.w < CELL
        and 0 <= want.h - have.h < CELL
    )


def focus_agrees() -> bool:
    """The window KWin has focused is the one the daemon last handled a focus change for."""
    log = daemon_log()
    focus_changes = [e.window for e in daemon.events(log) if e.kind == "FocusChange"]
    active = kde.active()
    if not focus_changes or active == "":
        return False
    return daemon.window_ids(log).get(kde.title(active)) == focus_changes[-1]


def answering() -> bool:
    """`komorebic monitor-information` is answered, with the session's one output."""
    status, out = kde.run("komorebic monitor-information")
    return status == 0 and json.loads(out)[0]["size"]["right"] == W


def parked() -> list[str]:
    """The titles of the windows that are on the parking desktop, which is the last desktop."""
    names = kde.desktop_names()
    if PARK not in names:
        return []
    park = names.index(PARK) + 1
    return sorted(
        t for t in ["win1", "win2"] if kde.wid(t) != "" and kde.desk(kde.wid(t)) == park
    )


def on_a_workspace_desktop() -> bool:
    """Every window that is open is on a desktop that is a workspace, not on the parking one."""
    return all(
        kde.desk(kde.wid(t)) in (1, 2) for t in ["win1", "win2"] if kde.wid(t) != ""
    )


def state_agrees() -> bool:
    """The window komorebi's state has focused is the one KWin has."""
    active = kde.active()
    return active != "" and kde.title(active) == kde.focused_title()


def at(title: str) -> tuple[int, int]:
    """Where KWin has the window: foot rounds its size to its cells, but not its position."""
    r = kde.rect(kde.wid(title))
    return (r.x, r.y)


def report(titles: list[str]) -> None:
    for title in titles:
        print(f"     {title}: asked {asked(title)}, has {kde.rect(kde.wid(title))}")


print("== 1. start")
kde.start_komorebi()
checks.check(
    "the daemon loaded its script into KWin",
    kde.wait_for(lambda: "loaded the KWin bridge script" in daemon_log(), timeout=30),
)
checks.check("KWin runs the script", kde.script_loaded())
checks.check(
    "the daemon is up and answering komorebic", kde.wait_for(answering, timeout=30)
)

print("== 2. three windows are tiled")
titles = ["win1", "win2", "win3"]
for title in titles:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=30)
    kde.wait_for(
        lambda title=title: title in daemon.window_ids(daemon_log()), timeout=10
    )
settled = kde.wait_for(lambda: all(where_asked(t) for t in titles), timeout=20)
report(titles)
checks.check("every window is where the daemon asked for it", settled)
rects = [kde.rect(kde.wid(t)) for t in titles]
for title, rect in zip(titles, rects, strict=True):
    checks.check(f"{title} is inside the screen", inside(rect))
checks.check("win1 and win2 do not overlap", not rects[0].overlaps(rects[1]))
checks.check("win1 and win3 do not overlap", not rects[0].overlaps(rects[2]))
checks.check("win2 and win3 do not overlap", not rects[1].overlaps(rects[2]))
checks.check(
    "the windows share the screen: none is full width", all(r.w < W - 20 for r in rects)
)
checks.check(
    "KWin and the daemon agree on the focused window", kde.wait_for(focus_agrees)
)
checks.check(
    "komorebi manages the three windows", kde.wait_for(lambda: kde.managed() == 3)
)
checks.check(
    "komorebi's state focuses the window KWin does", kde.wait_for(state_agrees)
)

print("== 3. the focus moves by hand")
kde.activate(kde.wid("win1"))
checks.check(
    "KWin focused win1", kde.wait_for(lambda: kde.title(kde.active()) == "win1")
)
checks.check("... and the daemon followed", kde.wait_for(focus_agrees))
checks.check("... and its state says so", kde.wait_for(state_agrees))

print("== 4. the focus moves on komorebic's commands")
kde.komorebic("focus", "right")
checks.check(
    "after 'focus right' KWin has given the focus to another window",
    kde.wait_for(lambda: kde.title(kde.active()) not in ("", "win1")),
)
checks.check(
    "... and komorebi's state focuses the same one", kde.wait_for(state_agrees)
)
kde.komorebic("focus", "left")
checks.check(
    "'focus left' gives it back to win1",
    kde.wait_for(lambda: kde.title(kde.active()) == "win1"),
)
checks.check("... and komorebi's state says so", kde.wait_for(state_agrees))
kde.komorebic("cycle-focus", "next")
checks.check(
    "'cycle-focus next' focuses win2, the next container in komorebi's order",
    kde.wait_for(lambda: kde.title(kde.active()) == "win2"),
)
checks.check("... and komorebi's state says so", kde.wait_for(state_agrees))
kde.komorebic("cycle-focus", "previous")
checks.check(
    "'cycle-focus previous' goes back to win1",
    kde.wait_for(lambda: kde.title(kde.active()) == "win1"),
)
checks.check("moving the focus moved no window", all(where_asked(t) for t in titles))

print("== 5. the windows move on komorebic's commands")
left, top_right = at("win1"), at("win2")
kde.komorebic("move", "right")
checks.check(
    "after 'move right' win1 and win2 have swapped tiles",
    kde.wait_for(lambda: at("win1") == top_right and at("win2") == left, timeout=20),
)
checks.check(
    "... every window is where the daemon asked for it",
    kde.wait_for(lambda: all(where_asked(t) for t in titles), timeout=20),
)
checks.check(
    "... and win1 keeps the focus",
    kde.wait_for(lambda: kde.title(kde.active()) == "win1" and state_agrees()),
)
kde.komorebic("cycle-move", "previous")
checks.check(
    "'cycle-move previous' puts win1 and win2 back",
    kde.wait_for(lambda: at("win1") == left and at("win2") == top_right, timeout=20),
)
report(titles)

print("== 6. monocle")
focused = kde.active()
tile = kde.rect(focused)
kde.komorebic("toggle-monocle")


def fills_the_work_area(wid: str) -> bool:
    r = kde.rect(wid)
    return r.w > W - 120 and r.h > H - 120


checks.check(
    "in monocle the focused window fills the work area",
    kde.wait_for(lambda: fills_the_work_area(focused), timeout=20),
)
checks.check(
    "komorebi's state has it as the monocle container",
    kde.wait_for(
        lambda: (
            kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0][
                "monocle_container"
            ]
            is not None
        )
    ),
)
checks.check(
    "... and it still has the focus", kde.title(kde.active()) == kde.title(focused)
)
kde.komorebic("toggle-monocle")
checks.check(
    "toggling back puts it on its tile",
    kde.wait_for(lambda: kde.rect(focused) == tile, timeout=20),
)
checks.check(
    "... and every window is where the daemon asked for it",
    kde.wait_for(lambda: all(where_asked(t) for t in titles), timeout=20),
)

kde.komorebic("monocle-focus-behaviour", "cycle")
kde.komorebic("toggle-monocle")
kde.wait_for(lambda: fills_the_work_area(focused), timeout=20)
kde.komorebic("focus", "right")
checks.check(
    "with the cycle behaviour, focusing in a direction gives another window the work area",
    kde.wait_for(
        lambda: kde.active() != focused and fills_the_work_area(kde.active()),
        timeout=20,
    ),
)
kde.komorebic("toggle-monocle")
kde.komorebic("monocle-focus-behaviour", "no-op")
checks.check(
    "the three windows are tiled again",
    kde.wait_for(
        lambda: (
            all(where_asked(t) for t in titles)
            and all(kde.rect(kde.wid(t)).w < W - 20 for t in titles)
        ),
        timeout=20,
    ),
)

print("== 7. minimize and restore")
kde.minimize(kde.wid("win2"))
two = ["win1", "win3"]
checks.check(
    "the other two windows take the screen",
    kde.wait_for(
        lambda: (
            all(where_asked(t) for t in two)
            and all(kde.rect(kde.wid(t)).w > W // 2 - 40 for t in two)
        ),
        timeout=20,
    ),
)
report(two)
kde.activate(kde.wid("win2"))
checks.check(
    "restored, the three windows are tiled again",
    kde.wait_for(
        lambda: (
            all(where_asked(t) for t in titles)
            and all(kde.rect(kde.wid(t)).w < W - 20 for t in titles)
        ),
        timeout=20,
    ),
)
report(titles)

print("== 8. close")
kde.close(kde.wid("win3"))
checks.check(
    "KWin closed win3", kde.wait_for(lambda: kde.wid("win3") == "", timeout=10)
)
checks.check(
    "the two that are left share the screen again",
    kde.wait_for(
        lambda: (
            all(where_asked(t) for t in ["win1", "win2"])
            and not kde.rect(kde.wid("win1")).overlaps(kde.rect(kde.wid("win2")))
        ),
        timeout=20,
    ),
)
report(["win1", "win2"])
checks.check(
    "komorebi manages the two that are left", kde.wait_for(lambda: kde.managed() == 2)
)
checks.check(
    "KWin carried out every command", "the bridge could not do" not in daemon_log()
)

print("== 9. a clean stop undoes what a monocle did")
kde.komorebic("toggle-monocle")
checks.check(
    "the window that is not in monocle is out of sight on a parking desktop",
    kde.wait_for(lambda: len(parked()) == 1),
)
kde.komorebic("stop")
exited = kde.wait_for(lambda: not kde.komorebi_running())
if not exited:
    print(kde.systemctl("status", "komorebi@main.service", "--no-pager", "-l"))
    print("\n".join(daemon_log().splitlines()[-15:]))
checks.check("the daemon exited", exited)
checks.check(
    "the parking desktop is gone", kde.wait_for(lambda: PARK not in kde.desktop_names())
)
checks.check(
    "and every window is on a workspace's desktop", kde.wait_for(on_a_workspace_desktop)
)

print("== 10. a crash with a monocle on leaves nothing stuck behind")
kde.start_komorebi()
checks.check("the daemon answers", kde.wait_for(answering, timeout=30))
checks.check(
    "it manages the two windows again", kde.wait_for(lambda: kde.managed() == 2)
)
kde.komorebic("toggle-monocle")
checks.check(
    "a window is parked and a parking desktop exists",
    kde.wait_for(lambda: len(parked()) == 1),
)
kde.kill_komorebi()
checks.check(
    "the daemon is dead and the parking desktop is still there, with a window on it",
    PARK in kde.desktop_names() and len(parked()) == 1,
)
kde.start_komorebi()
checks.check(
    "the next start removed the stale parking desktop",
    kde.wait_for(lambda: answering() and PARK not in kde.desktop_names()),
)
checks.check("and brought the parked window back", kde.wait_for(on_a_workspace_desktop))
checks.check(
    "it manages the two windows again", kde.wait_for(lambda: kde.managed() == 2)
)
kde.komorebic("toggle-monocle")
checks.check(
    "after a crash and a restart a monocle still hides the other window",
    kde.wait_for(lambda: len(parked()) == 1),
)
kde.komorebic("toggle-monocle")
checks.check(
    "and turned off, nothing is out of sight", kde.wait_for(lambda: parked() == [])
)

print("== 11. KWin restarts under the daemon")
kde.restart_kwin()
checks.check(
    "the daemon loaded its script into the new KWin",
    kde.wait_for(
        lambda: daemon_log().count("loaded the KWin bridge script") == 2, timeout=30
    ),
)
checks.check(
    "komorebi let go of the windows of the old KWin",
    kde.wait_for(lambda: kde.managed() == 0),
)
kde.spawn("foot -T afterwards sleep 3000")
kde.wait_for(lambda: kde.wid("afterwards") != "", timeout=30)
checks.check(
    "a window opened in the new KWin is managed",
    kde.wait_for(lambda: kde.managed() == 1),
)
checks.check(
    "... and tiled on its own",
    kde.wait_for(
        lambda: (
            where_asked("afterwards") and kde.rect(kde.wid("afterwards")).w > W - 100
        ),
        timeout=20,
    ),
)
report(["afterwards"])

print(
    "== 12. tiling, focus, moving, stacks, monocle, workspaces and closing, through the neutral platform"
)
# the scenarios of komorebi_vm.core.scenarios are what runs on Windows too: it asks the
# operating system where the windows are, and not the daemon's log
kde.close(kde.wid("afterwards"))
checks.check(
    "the window of the last section is closed", kde.wait_for(lambda: kde.managed() == 0)
)
platform = KdePlatform(kde, (W, H))
neutral = tiling.run(platform, checks, titles=("neutral1", "neutral2", "neutral3"))
focus.run(platform, checks, neutral)
moving.run(platform, checks, neutral)
stacks.run(platform, checks, neutral)

# what `komorebic state` prints, kept as a file of the run so that the unit tests of the neutral
# core (tests/fixtures) are of a real state and not of one written from the serializer
(machine.out_dir / "state-kfde-tiled.json").write_text(
    json.dumps(kde.state(), indent=2)
)
platform.activate(neutral[-1])
kde.komorebic("stack", "left")
checks.check(
    "the windows are stacked",
    kde.wait_for(lambda: sorted(map(len, kde.containers())) == [1, 2]),
)
(machine.out_dir / "state-kfde-stack.json").write_text(
    json.dumps(kde.state(), indent=2)
)
kde.komorebic("unstack")
checks.check(
    "and unstacked",
    kde.wait_for(lambda: sorted(map(len, kde.containers())) == [1, 1, 1]),
)
layouts.run(platform, checks, neutral)
floating.run(platform, checks, neutral)
minimizing.run(platform, checks, neutral)
maximizing.run(platform, checks, neutral)
resize_commands.run(platform, checks, neutral)
promoting.run(platform, checks, neutral)
flipping.run(platform, checks, neutral)
padding.run(platform, checks, neutral)
monocle.run(platform, checks, neutral)
workspace_moves.run(platform, checks, neutral)
workspaces.run(platform, checks, neutral)
# the third window is on workspace 1 now: the first is closed, and the second takes its room
closing.run(platform, checks, neutral, Client("neutral1"))

# the rules start from a workspace with no window: close what is left, wherever it is
for title in ("neutral2", "neutral3"):
    platform.close(Client(title))
checks.check("no window is left", kde.wait_for(lambda: kde.managed_all() == 0, timeout=20))
platform.komorebic("focus-workspace", "0")
rules.run(platform, checks)

print("== 12a. where a window that opens goes, from the placement rules of the config")
placement.run(platform, checks)
checks.check("no window is left", kde.wait_for(lambda: kde.managed_all() == 0, timeout=20))

print("== 12a2. the workspace rules of the config, with a window open before the daemon")
startup_rules.run(platform, checks)
checks.check("no window is left", kde.wait_for(lambda: kde.managed_all() == 0, timeout=20))
# a window for the stop to leave behind
platform.spawn("afterwards")
checks.check("it is managed", kde.wait_for(lambda: kde.managed_all() == 1, timeout=20))

print("== 12b. a daemon that stops and starts again takes the windows up")
lifecycle.run(platform, checks, [Client("afterwards"), platform.spawn("beside")])
checks.check("the daemon answers", kde.wait_for(answering, timeout=30))
platform.close(Client("after-the-restart"))
platform.close(Client("beside"))
checks.check("one window is left", kde.wait_for(lambda: kde.managed_all() == 1, timeout=20))

print("== 12c. a window sent to a monitor that does not exist")
# KNOWN FAILURE, to follow up on: move_container_to_monitor takes the container out before it
# looks the monitor up. See docs/dev/BEHAVIOUR.md. The window is lost, so this stays last.
commands.unknown_monitor(
    platform,
    checks,
    [Client("afterwards")],
    known_failure="the container is removed before the monitor is looked up, on both ports",
)

print("== 13. komorebic stop")
kde.komorebic("stop")
checks.check("the daemon exited", kde.wait_for(lambda: not kde.komorebi_running()))
checks.check(
    "its socket is gone",
    kde.run(f"test -e {kde.runtime}/komorebi/komorebi.sock")[0] != 0,
)
checks.check(
    "the script is no longer in KWin", kde.wait_for(lambda: not kde.script_loaded())
)
checks.check("the window stays", kde.wid("afterwards") != "")

kde.stop()
checks.finish()
