# `initial_window_placement_rules` in a real (headless) KWin: where in the layout a window that
# opens goes. The daemon reads the rules from a `komorebi.json` it is started with, and the rules
# match a window by what it is called: its program (`Exe`), its Wayland app id (`Class`) and its
# title. `komorebi/tests/placement.rs` checks the same against the fake; this checks that the names
# the rules match are the ones a real window has, and that the places of a file are the places the
# windows get.
import json
import shlex
import time
from typing import Any

from komorebi_vm import Checks, Kde

CONFIG = "/tmp/placement.json"

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)
opened: list[str] = []


def order() -> list[str]:
    """The title of the window of each container of the first workspace, in the layout's order."""
    return [titles[0] for titles in kde.containers()]


def open_window(title: str, app_id: str = "foot") -> None:
    """Opens a window, and waits until the daemon has put it in a container."""
    kde.spawn(f"foot -a {shlex.quote(app_id)} -T {shlex.quote(title)} sleep 3000")
    opened.append(title)
    kde.wait_for(lambda: kde.wid(title) != "", timeout=30)
    kde.wait_for(lambda: kde.managed() == len(opened), timeout=20)
    # the window that has just opened has the focus once the daemon has told KWin so
    kde.wait_for(lambda: kde.focused_title() == title, timeout=20)
    time.sleep(0.5 * SCALE)


def fresh(rules: Any) -> None:
    """Starts the daemon again, with `rules` as the placement rules of the workspace's windows.

    The windows of the run before are closed. `None` leaves the key out of the file.
    """
    if kde.komorebi_running():
        kde.komorebic("stop")
        kde.wait_for(lambda: not kde.komorebi_running(), timeout=30)
    for title in opened:
        wid = kde.wid(title)
        if wid != "":
            kde.close(wid)
    kde.wait_for(lambda: all(kde.wid(title) == "" for title in opened), timeout=30)
    opened.clear()

    workspace: dict[str, Any] = {"name": "a", "layout": "BSP"}
    if rules is not None:
        workspace["initial_window_placement_rules"] = rules
    config = {
        "default_workspace_padding": 10,
        "default_container_padding": 10,
        "monitors": [{"workspaces": [workspace]}],
    }
    kde.sh(f"printf %s {shlex.quote(json.dumps(config))} > {CONFIG}")
    kde.start_komorebi("--config", CONFIG)
    checks.check(
        "the daemon answers komorebic",
        kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=30),
    )


print("== 1. no rules: after the one in focus")
fresh(None)
for title in ("a", "b", "c"):
    open_window(title)
checks.check(f"a, b, c ({order()})", order() == ["a", "b", "c"])

print("== 2. every window to the primary tile")
fresh("Primary")
for title in ("a", "b", "c"):
    open_window(title)
checks.check(f"c, b, a ({order()})", order() == ["c", "b", "a"])

print("== 3. by the number of the place, counted from 1, as a number and as text")
fresh(2)
for title in ("a", "b", "c"):
    open_window(title)
checks.check(f"a, c, b ({order()})", order() == ["a", "c", "b"])
fresh("2")
for title in ("a", "b", "c"):
    open_window(title)
checks.check(
    f"text that is a number is the same place: a, c, b ({order()})", order() == ["a", "c", "b"]
)

print("== 4. by the app id and the title")
fresh(
    {
        "Primary": {"kind": "Class", "id": "special", "matching_strategy": "Equals"},
        "Last": [{"kind": "Title", "id": "chat", "matching_strategy": "Contains"}],
    }
)
for title in ("one", "two"):
    open_window(title)
open_window("three", "special")
checks.check(
    f"the window with the app id special is first ({order()})", order() == ["three", "one", "two"]
)
open_window("a chat room")
checks.check(
    f"the chat room is last ({order()})", order() == ["three", "one", "two", "a chat room"]
)
open_window("four")
checks.check(
    f"a window no rule matches goes after the one in focus, the chat room ({order()})",
    order() == ["three", "one", "two", "a chat room", "four"],
)

print("== 5. by the program, with the name of a Windows executable")
fresh({"Primary": {"kind": "Exe", "id": "foot.exe", "matching_strategy": "Equals"}})
for title in ("a", "b", "c"):
    open_window(title)
checks.check(
    f"foot.exe is foot, so every window is a primary one: c, b, a ({order()})",
    order() == ["c", "b", "a"],
)

kde.komorebic("stop")
checks.check("a clean stop", kde.wait_for(lambda: not kde.komorebi_running(), timeout=30))
kde.stop()
checks.finish()
