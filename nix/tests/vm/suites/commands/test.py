# Every layout, and a sweep of komorebic's commands, against a real headless KWin with two outputs.
#
# After each command the same things have to be true: komorebi is alive, it can still say what it
# knows (`komorebic state`), and every window it has is a window KWin has. After each layout, the
# windows are inside the outputs and do not lie on each other. A command is also expected to be
# understood (exit status 0). The Rust tests check what each command does against the fake; this
# suite checks that none of them breaks the daemon in a real session, and that what the daemon
# asks KWin for is what KWin carries out. Every command `komorebic` has is in the sweep, but
# `minimize` and `unmanage`, which take a window out of komorebi's hands and have a section of
# their own.
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
stack left
cycle-stack next
cycle-stack previous
cycle-stack-index next
focus-stack-window 0
unstack
stack-all
unstack-all
resize-edge left increase
resize-edge right decrease
resize-edge up increase
resize-edge down decrease
resize-axis horizontal increase
resize-axis vertical decrease
resize-delta 50
resize-delta 100
promote
promote-swap
promote-focus
promote-window left
retile
flip-layout horizontal
flip-layout vertical
flip-layout horizontal-and-vertical
flip-layout horizontal-and-vertical
cycle-layout next
cycle-layout previous
layout-ratios --columns 0.6 0.4
change-layout columns
scrolling-layout-columns 2
change-layout bsp
preselect-direction left
cancel-preselect
toggle-monocle
toggle-monocle
toggle-maximize
toggle-maximize
toggle-float
toggle-float
toggle-lock
toggle-lock
toggle-tiling
toggle-tiling
toggle-pause
toggle-pause
toggle-workspace-layer
toggle-workspace-layer
toggle-window-container-behaviour
toggle-window-container-behaviour
toggle-float-override
toggle-float-override
toggle-workspace-window-container-behaviour
toggle-workspace-window-container-behaviour
toggle-workspace-float-override
toggle-workspace-float-override
ensure-named-workspaces 0 one two three
workspace-name 0 0 one
ensure-workspaces 0 3
workspace-tiling 0 0 disable
workspace-tiling 0 0 enable
named-workspace-tiling one disable
named-workspace-tiling one enable
focus-workspace 1
focus-workspace 0
focus-last-workspace
focus-last-workspace
focus-workspaces 1
focus-workspaces 0
cycle-workspace next
cycle-workspace previous
cycle-empty-workspace next
focus-workspace 0
focus-named-workspace two
focus-named-workspace one
send-to-workspace 1
focus-workspace 1
move-to-workspace 0
focus-workspace 0
cycle-send-to-workspace next
focus-workspace 1
cycle-move-to-workspace previous
focus-workspace 0
send-to-last-workspace
move-to-last-workspace
focus-workspace 0
new-workspace
close-workspace
focus-workspace 0
workspace-layout 0 0 columns
workspace-layout 0 0 bsp
named-workspace-layout one columns
named-workspace-layout one bsp
workspace-layout-rule 0 0 2 rows
workspace-layout-rule 0 0 3 columns
clear-workspace-layout-rules 0 0
named-workspace-layout-rule one 3 columns
clear-named-workspace-layout-rules one
workspace-layout 0 0 bsp
focus-monitor 1
focus-monitor 0
cycle-monitor next
cycle-monitor previous
focus-monitor-workspace 1 1
focus-monitor-workspace 0 0
focus-monitor-at-cursor
focus-monitor 0
send-to-named-workspace two
focus-named-workspace two
move-to-named-workspace one
focus-named-workspace one
send-to-monitor 1
focus-monitor 1
move-to-monitor 0
focus-monitor 0
cycle-send-to-monitor next
focus-monitor 1
cycle-move-to-monitor next
focus-monitor 0
send-to-monitor-workspace 1 1
focus-monitor-workspace 1 1
move-to-monitor-workspace 0 0
focus-monitor 0
focus-workspace 0
move-workspace-to-monitor 1
cycle-move-workspace-to-monitor next
swap-workspaces-with-monitor 1
swap-workspaces-with-monitor 1
global-work-area-offset 10 10 20 20
global-work-area-offset 0 0 0 0
monitor-work-area-offset 0 10 10 20 20
monitor-work-area-offset 0 0 0 0 0
workspace-work-area-offset 0 0 10 10 20 20
workspace-work-area-offset 0 0 0 0 0 0
toggle-window-based-work-area-offset
toggle-window-based-work-area-offset
focused-workspace-container-padding 20
focused-workspace-container-padding 10
focused-workspace-padding 20
focused-workspace-padding 10
adjust-container-padding increase 5
adjust-container-padding decrease 5
adjust-workspace-padding increase 5
adjust-workspace-padding decrease 5
container-padding 0 0 12
container-padding 0 0 10
workspace-padding 0 0 12
workspace-padding 0 0 10
named-workspace-container-padding one 12
named-workspace-container-padding one 10
named-workspace-padding one 12
named-workspace-padding one 10
cross-monitor-move-behaviour insert
cross-monitor-move-behaviour swap
toggle-cross-monitor-move-behaviour
toggle-cross-monitor-move-behaviour
monocle-focus-behaviour cycle
toggle-monocle-focus-behaviour
toggle-monocle-focus-behaviour
unmanaged-window-operation-behaviour op
unmanaged-window-operation-behaviour no-op
unmanaged-window-operation-behaviour op
display-index-preference 0 Virtual-0
mouse-follows-focus enable
mouse-follows-focus disable
toggle-mouse-follows-focus
toggle-mouse-follows-focus
border enable
border-width 8
border-offset 1
border-colour --window-kind single 10 20 30
border disable
animation enable
animation-duration 120
animation-fps 30
animation disable
session-float-rule
session-float-rules
clear-session-float-rules
toggle-float
ignore-rule exe nonexistent
manage-rule exe nonexistent
initial-workspace-rule exe nonexistent 0 1
workspace-rule exe nonexistent 0 1
initial-named-workspace-rule exe nonexistent two
named-workspace-rule exe nonexistent two
enforce-workspace-rules
clear-workspace-rules 0 1
clear-named-workspace-rules two
clear-all-workspace-rules
manage
close
retile
replace-configuration /tmp/e2e-config.json
eager-focus foot
"""
kde.sh(
    "printf %s "
    + shlex.quote('{ "monitors": [ { "workspaces": [ { "name": "again", "layout": "BSP" } ] } ] }')
    + " > /tmp/e2e-config.json"
)
for line in SWEEP.strip().splitlines():
    status, out = kde.run(shlex.join(["komorebic", *line.split()]))
    if status != 0:
        checks.bad(f"komorebic {line}: exit {status}: {' '.join(out.splitlines()[:2])}")
        continue
    if not kde.wait_for(lambda: healthy() == 0, timeout=5):
        checks.bad(f"komorebic {line}: komorebi is not healthy afterwards ({healthy()})")
        continue
    checks.ok(f"komorebic {line}")

first_workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
checks.check(
    "replace-configuration made the window manager again from the file",
    first_workspace["name"] == "again",
)

print("== 3b. minimize, unmanage and manage the focused window")
focused = kde.active()
title = kde.title(focused)
before = kde.managed()
kde.wait_for(lambda: kde.focused_title() == title, timeout=10)
kde.komorebic("minimize")
checks.check(
    f"minimize: komorebi lets go of {title} when KWin minimizes it",
    kde.wait_for(lambda: kde.managed() == before - 1, timeout=10)
    and kde.wid(title) != "",
)
kde.activate(focused)
checks.check(
    f"{title}, restored, is tiled again",
    kde.wait_for(lambda: kde.managed() == before and healthy() == 0, timeout=10),
)
kde.wait_for(lambda: kde.focused_title() == title, timeout=10)
kde.komorebic("unmanage")
checks.check(
    f"unmanage: komorebi lets go of {title}, which stays open",
    kde.wait_for(lambda: kde.managed() == before - 1, timeout=10) and kde.wid(title) != "",
)


def centered() -> bool:
    r = kde.rect(focused)
    return abs(r.x + r.w // 2 - W // 2) <= CELL and abs(r.y + r.h // 2 - H // 2) <= CELL


checks.check(f"{title} is in the middle of the work area", kde.wait_for(centered, timeout=10))
kde.komorebic("manage")
checks.check(
    f"manage: {title} is tiled again",
    kde.wait_for(lambda: kde.managed() == before and healthy() == 0, timeout=10),
)

print("== 3c. a rule that matches a title decides when the window has its title")
kde.komorebic("ignore-rule", "title", "pending-rule")
before_all = kde.managed_all()


def ignored(title: str) -> bool:
    """True once the daemon has logged that it leaves the window titled `title` alone."""
    return any(
        "should not be managed" in line and f"title: {title}," in line
        for line in kde.komorebi_log().splitlines()
    )


# foot is titled pending-rule when it opens, and gives itself another title after 4 seconds
rename = 'sleep 4; printf "\\033]0;renamed-rule\\007"; sleep 3000'
kde.spawn(f"foot -T pending-rule sh -c {shlex.quote(rename)}")
checks.check(
    "a window the ignore rule matches by its title is left alone",
    kde.wait_for(lambda: ignored("pending-rule"), timeout=20) and kde.managed_all() == before_all,
)
checks.check(
    "once it has a title the rule does not match, it is tiled",
    kde.wait_for(
        lambda: kde.wid("renamed-rule") != "" and kde.managed_all() == before_all + 1,
        timeout=20,
    )
    and healthy() == 0,
)
# the daemon's log names a window by the title it had when it was focused, which for this one is
# the first: close it, so that the last check finds every window it looks for in the log
kde.close(kde.wid("renamed-rule"))
checks.check(
    "closed, it leaves komorebi",
    kde.wait_for(lambda: kde.wid("renamed-rule") == "" and healthy() == 0, timeout=10),
)

print("== 4. what komorebic only prints")
for args in [
    "state",
    "global-state",
    "visible-windows",
    "monitor-information",
    "data-directory",
    "configuration",
    "generate-static-config",
    "static-config-schema",
    "application-specific-configuration-schema",
    "notification-schema",
    "socket-schema",
    "session-float-rules",
]:
    checks.check(f"komorebic {args}", kde.run(f"komorebic {args}")[0] == 0)
for query in [
    "focused-monitor-index",
    "focused-workspace-index",
    "focused-container-index",
    "focused-window-index",
    "focused-workspace-name",
    "focused-workspace-layout",
    "focused-container-kind",
    "version",
]:
    status, out = kde.run(f"komorebic query {query}")
    checks.check(f"query {query} answers", status == 0 and out.strip() != "")
checks.check(
    "quick-save-resize and quick-load-resize",
    kde.run("komorebic quick-save-resize && komorebic quick-load-resize")[0] == 0,
)
checks.check(
    "save-resize and load-resize",
    kde.run(
        "komorebic save-resize /tmp/e2e-resize.json && komorebic load-resize /tmp/e2e-resize.json"
    )[0]
    == 0,
)

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
