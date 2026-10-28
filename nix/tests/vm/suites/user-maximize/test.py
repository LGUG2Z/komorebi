# A window the user maximizes, in a real headless KWin. The user's title bar maximizes a window
# without asking komorebi: KWin does it and reports it, and komorebi has to take the window out of
# the layout as it does for `toggle-maximize`, leave the layout alone while the window is maximized,
# and put the window back in its tile when it is restored, by the user or by komorebi.
# `komorebi/tests/user_maximize.rs` checks what the window manager does against the fake; this
# checks that KWin reports the user's maximize, and the restore, the way komorebi reads them.
import json
import shlex
import time
from typing import Any

from komorebi_vm import Checks, Kde, Rect, daemon

W, H = 1920, 1080
# The windows are tiled with a gap of 10 pixels around the workspace and between the tiles: a tile
# has padding, and a maximized window has none. The windows are not animated.
CONFIG = "/tmp/user-maximize-config.json"

start_all()
machine.wait_for_unit("multi-user.target")
machine.wait_for_unit("user@1000.service")

checks = Checks()
kde = Kde.headless(machine, "main", size=(W, H), scale=SCALE)


def workspace() -> dict[str, Any]:
    return kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]


def containers() -> int:
    return len(workspace()["containers"]["elements"])


def maximized_title() -> str | None:
    window = workspace()["maximized_window"]
    return None if window is None else window["details"]["title"]


def fills(r: Rect) -> bool:
    """The work area is the whole output: a virtual KWin has no panel."""
    return r.w >= W - 40 and r.h >= H - 80


def rect_of(title: str) -> Rect:
    return kde.rect(kde.wid(title))


def open_window(title: str) -> None:
    kde.spawn(f"foot -T {title} sleep 3000")
    kde.wait_for(lambda: kde.wid(title) != "", timeout=30)


print("== three windows, tiled")
kde.sh(
    "printf %s "
    + shlex.quote(
        json.dumps(
            {
                "default_workspace_padding": 10,
                "default_container_padding": 10,
                "animation": {"enabled": False},
            }
        )
    )
    + f" > {CONFIG}"
)
kde.start_komorebi("--config", CONFIG)
kde.wait_for(lambda: kde.run("komorebic monitor-information")[0] == 0, timeout=30)
for title in ("um-a", "um-b", "um-c"):
    open_window(title)
checks.check("komorebi has three tiles", kde.wait_for(lambda: containers() == 3, timeout=20))
# The bridge asks again for the position of a window that is not where komorebi put it for a second
# after it did (PLATFORM.md, "How clients answer a frame"), which would put a window that is
# maximized in that second back where its tile was: wait that second out, as a person would.
kde.wait_for(lambda: kde.focused_title() == "um-c", timeout=20)
time.sleep(3 * SCALE)
tiles = {title: rect_of(title) for title in ("um-a", "um-b", "um-c")}
print(f"     the tiles are {tiles}")

print("== 1. the user maximizes a window with its title bar")
kde.maximize(kde.wid("um-b"))
checks.check(
    "komorebi has it as its maximized window",
    kde.wait_for(lambda: maximized_title() == "um-b", timeout=20),
)
checks.check(
    f"it fills the work area ({rect_of('um-b')})",
    kde.wait_for(lambda: fills(rect_of("um-b")), timeout=10),
)
checks.check("and its tile is gone: two are left", containers() == 2)
checks.check(
    "komorebi handled it as a change of the window's maximized state",
    any(
        event.kind == "MaximizeChange" and event.notification == "WindowMaximizedChanged"
        for event in daemon.events(kde.komorebi_log())
    ),
)
checks.check("it still has all three windows", kde.managed_all() == 3)

print("== 2. what is asked of the layout does nothing")
beside = {title: rect_of(title) for title in ("um-a", "um-c")}
for command in [
    "move left",
    "move right",
    "move up",
    "move down",
    "stack left",
    "stack right",
    "unstack",
    "cycle-move next",
    "promote",
    "toggle-float",
    "toggle-monocle",
    "resize-edge left increase",
    "send-to-workspace 1",
    "move-to-workspace 1",
    "toggle-lock",
]:
    kde.komorebic(*command.split())
time.sleep(1 * SCALE)
checks.check(
    "it is still maximized, and still komorebi's maximized window",
    fills(rect_of("um-b")) and maximized_title() == "um-b",
)
checks.check(
    "the others did not move, and nothing floats or is in the monocle",
    {title: rect_of(title) for title in ("um-a", "um-c")} == beside
    and containers() == 2
    and workspace()["floating_windows"]["elements"] == []
    and workspace()["monocle_container"] is None,
)
checks.check(
    "komorebi noted that it ignored them",
    "ignoring" in kde.komorebi_log() and "the window in front is maximized" in kde.komorebi_log(),
)

print("== 3. toggle-maximize restores it")
kde.komorebic("toggle-maximize")
checks.check(
    f"it is back in its tile ({tiles['um-b']})",
    kde.wait_for(lambda: rect_of("um-b") == tiles["um-b"], timeout=20),
)
checks.check(
    "and nothing is maximized",
    maximized_title() is None and containers() == 3,
)

print("== 4. the user restores it with the title bar")
kde.maximize(kde.wid("um-c"))
checks.check(
    "maximized again",
    kde.wait_for(lambda: maximized_title() == "um-c" and containers() == 2, timeout=20),
)
kde.unmaximize(kde.wid("um-c"))
checks.check(
    f"restored by the user, it is back in its tile ({tiles['um-c']})",
    kde.wait_for(lambda: rect_of("um-c") == tiles["um-c"], timeout=20),
)
checks.check(
    "and nothing is maximized",
    kde.wait_for(lambda: maximized_title() is None and containers() == 3, timeout=10),
)

kde.komorebic("stop")
kde.wait_for(lambda: not kde.komorebi_running())
kde.stop()
checks.finish()
