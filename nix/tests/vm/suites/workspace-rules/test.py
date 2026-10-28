# Workspace rules in a real KWin: a rule sends the windows of an application to a workspace, which
# is a virtual desktop, so that a window that opens on the desktop in front leaves the workspace
# in front and is hidden, on the parking desktop, until its workspace is shown. What only a real
# KWin can tell: that the window is out of sight on the parking desktop while the desktop in front
# stays showing, that it is on the desktop of the workspace the rule names and tiled when that
# workspace is shown, that an initial rule sends a window once and a permanent rule sends it back,
# and that the rules a `komorebi.json` gives apply to a window that was open before the daemon
# started. `komorebi/tests/workspace_rules.rs` checks the same against the fake.
import json
import shlex

from komorebi_vm import Checks, Kde, daemon

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)
# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24


def open_window(title: str) -> str:
    kde.spawn(f"foot -T {shlex.quote(title)} sleep 3000")
    kde.wait_for(lambda: kde.wid(title) != "", timeout=30)
    return kde.wid(title)


def workspace_of(title: str) -> int | None:
    """The workspace of the first monitor that has a window with the title in a container."""
    elements = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"]
    for index, workspace in enumerate(elements):
        for container in workspace["containers"]["elements"]:
            if any(w["details"]["title"] == title for w in container["windows"]["elements"]):
                return index
    return None


def shown_workspace() -> int:
    return kde.state()["monitors"]["elements"][0]["workspaces"]["focused"]


def park_desktop() -> int:
    """The number of the parking desktop, counted from 1 as kdotool counts, or 0 for none."""
    names = kde.desktop_names()
    return names.index("komorebi-park") + 1 if "komorebi-park" in names else 0


def parked(wid: str) -> bool:
    """Whether the window is on the parking desktop, out of sight."""
    return park_desktop() != 0 and kde.desk(wid) == park_desktop()


print("== 1. a rule that was made while the daemon runs")
kde.start_komorebi()
checks.check(
    "the daemon answers komorebic",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=30),
)
kde.komorebic("ensure-named-workspaces", "0", "web", "code", "chat")
kde.komorebic("initial-workspace-rule", "title", "sent-once", "0", "1")
kde.komorebic("named-workspace-rule", "title", "sent-always", "chat")

stay = open_window("stays here")
once = open_window("sent-once one")
checks.check(
    "the window the rule names is on workspace 1, whichever way it got there",
    kde.wait_for(lambda: workspace_of("sent-once one") == 1, timeout=30),
)
checks.check(
    "... and it is out of sight on the parking desktop, while desktop 1 is still showing",
    kde.wait_for(lambda: parked(once) and kde.shown() == 1 and shown_workspace() == 0),
)
checks.check(
    "the other window is where it opened",
    workspace_of("stays here") == 0 and kde.desk(stay) == 1,
)

kde.komorebic("focus-workspace", "1")
checks.check("workspace 1 is shown", kde.wait_for(lambda: kde.shown() == 2))
checks.check(
    "and the window is back, on KWin's desktop 2, the desktop of its workspace",
    kde.wait_for(lambda: kde.desk(once) == 2 and not parked(once)),
)


def tiled(title: str) -> bool:
    want, wid = daemon.asked(kde.komorebi_log(), title), kde.wid(title)
    if want is None or wid == "":
        return False
    have = kde.rect(wid)
    return (have.x, have.y) == (want.x, want.y) and 0 <= want.w - have.w < CELL


checks.check(
    "the window is tiled where komorebi asked",
    kde.wait_for(lambda: tiled("sent-once one")),
)

print("== 2. an initial rule sends a window once, a permanent rule every time")
kde.komorebic("move-to-workspace", "0")
checks.check(
    "the user takes the window back to workspace 0",
    kde.wait_for(lambda: workspace_of("sent-once one") == 0 and kde.desk(once) == 1),
)
always = open_window("sent-always one")
checks.check(
    "a window the permanent rule names goes to the workspace named chat (2), out of sight",
    kde.wait_for(lambda: workspace_of("sent-always one") == 2 and parked(always)),
)
checks.check(
    "the window the initial rule sent is where the user put it",
    workspace_of("sent-once one") == 0,
)
kde.komorebic("focus-workspace", "2")
checks.check(
    "showing chat brings the window back to KWin's desktop 3",
    kde.wait_for(lambda: kde.desk(always) == 3 and not parked(always)),
)
kde.komorebic("move-to-workspace", "0")
# a new window is an event, and every event looks at the rules first
open_window("poke one")
checks.check(
    "the permanent rule sends the window back at the next event",
    kde.wait_for(lambda: workspace_of("sent-always one") == 2, timeout=30),
)
kde.komorebic("enforce-workspace-rules")
checks.check(
    "enforce-workspace-rules sends the initial rule's window again",
    kde.wait_for(lambda: workspace_of("sent-once one") == 1, timeout=30),
)

print("== 3. the rules can all be taken away")
kde.komorebic("clear-all-workspace-rules")
state_rules = json.loads(kde.komorebic("global-state"))["workspace_rules"]
checks.check("no rule is left", state_rules == [])
later = open_window("sent-once two")
checks.check(
    "a window that opens now stays on the workspace in front",
    kde.wait_for(lambda: workspace_of("sent-once two") == shown_workspace(), timeout=30)
    and kde.desk(later) == kde.shown(),
)

print("== 4. the rules of komorebi.json, with a window that was open before the daemon")
kde.komorebic("stop")
kde.wait_for(lambda: not kde.komorebi_running(), timeout=30)
CONFIG = "/tmp/rules-config.json"
kde.sh(
    "printf %s "
    + shlex.quote(
        json.dumps(
            {
                "monitors": [
                    {
                        "workspaces": [
                            {"name": "web", "layout": "BSP"},
                            {
                                "name": "code",
                                "layout": "BSP",
                                "workspace_rules": [
                                    {
                                        "kind": "Title",
                                        "id": "stays here",
                                        "matching_strategy": "StartsWith",
                                    }
                                ],
                            },
                        ]
                    }
                ]
            }
        )
    )
    + f" > {CONFIG}"
)
kde.start_komorebi("--config", CONFIG)
checks.check(
    "the daemon answers komorebic again",
    kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=30),
)
checks.check(
    "the window that was open when it started is on the workspace the file names, out of sight",
    kde.wait_for(lambda: workspace_of("stays here") == 1 and parked(stay), timeout=30),
)

kde.komorebic("stop")
checks.check("a clean stop", kde.wait_for(lambda: not kde.komorebi_running()))
kde.stop()
checks.finish()
