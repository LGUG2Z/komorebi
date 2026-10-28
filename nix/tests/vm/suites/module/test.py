# The NixOS module runs komorebi in a real Plasma session: nothing in this test starts the daemon.
# What only a real session can tell: that logging in starts the module's user unit through
# `plasma-workspace.target`, that the daemon it runs reaches KWin from there and tiles a window
# opened in the session, that it runs with the module's log level, its `configFile` and the PATH
# of the user's session, that systemd starts it again when it is killed, and that a clean stop is
# not a failure. Once the unit is stopped, the daemon is started with a `komorebi.json` of the
# user's, which it follows when the file changes.
# The options and the unit the module writes are checked without a machine by `nixos-module-eval`
# (`nix/tests/nixos-module.nix`).
import json
import shlex

from komorebi_vm import Checks, Kde, Rect, daemon

UNIT = "komorebi.service"
# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24
# the daemon's log of the day, without colors, which the unit's journal has
LOG = "/home/komorebi/.local/share/komorebi/komorebi_plaintext.log.*"

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def active() -> bool:
    return kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0


def failed() -> bool:
    return kde.run(f"systemctl --user is-failed --quiet {UNIT}")[0] == 0


def main_pid() -> str:
    return kde.sh(f"systemctl --user show --property MainPID --value {UNIT}").strip()


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def managed_titles() -> list[str]:
    """The titles of the windows komorebi has on the first workspace of the first monitor."""
    workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
    return [
        window["details"]["title"]
        for container in workspace["containers"]["elements"]
        for window in container["windows"]["elements"]
    ]


def daemon_log() -> str:
    return machine.execute(f"cat {LOG}")[1]


print("== 1. logging in starts komorebi")
checks.check("the session started the module's unit", kde.wait_for(active, timeout=60))
checks.check("its daemon answers komorebic", kde.wait_for(answering, timeout=60))

pid = main_pid()
command = kde.sh(f"tr '\\0' ' ' < /proc/{pid}/cmdline").split()
print(f"     the unit runs: {' '.join(command)}")
checks.check("the unit runs bin/komorebi", command[0].endswith("/bin/komorebi"))
checks.check("... at the module's log level", command[1:3] == ["--log-level", "debug"])
checks.check(
    "... with the module's configFile as its --config",
    len(command) == 5 and command[3] == "--config" and command[4].endswith("-komorebi.json"),
)
environment = kde.sh(f"tr '\\0' '\\n' < /proc/{pid}/environ").splitlines()
path = next((line[len("PATH=") :] for line in environment if line.startswith("PATH=")), "")
checks.check(
    "... with the PATH of the user's session",
    path.split(":")
    == ["/run/wrappers/bin", "/etc/profiles/per-user/komorebi/bin", "/run/current-system/sw/bin"],
)

print("== 1b. the daemon is configured by the file")
workspaces = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"]
checks.check(
    "its workspaces are the ones the file names",
    [workspace["name"] for workspace in workspaces] == ["web", "code"],
)
checks.check(
    "KWin has a virtual desktop for each of them",
    kde.wait_for(lambda: kde.desktops() >= 2, timeout=30),
)
checks.check(
    "the settings the daemon says it has are the file's",
    json.loads(kde.komorebic("generate-static-config"))["default_workspace_padding"] == 40,
)

print("== 2. a window opened in the session is tiled")
width = json.loads(kde.komorebic("monitor-information"))[0]["size"]["right"]


def asked() -> Rect | None:
    return daemon.asked(daemon_log(), "tiled")


def tiled() -> bool:
    """KWin has the window where komorebi asked, and that is a tile across the screen."""
    want, wid = asked(), kde.wid("tiled")
    if want is None or wid == "":
        return False
    have = kde.rect(wid)
    return (
        (have.x, have.y) == (want.x, want.y)
        and 0 <= want.w - have.w < CELL
        and 0 <= want.h - have.h < CELL
        and want.w > width // 2
    )


kde.spawn("foot -T tiled sleep 3000")
checks.check("the window opens", kde.wait_for(lambda: kde.wid("tiled") != "", timeout=60))
checks.check("komorebi manages it", kde.wait_for(lambda: "tiled" in managed_titles(), timeout=30))
checks.check("KWin has it on its tile", kde.wait_for(tiled, timeout=30))
print(f"     asked {asked()}, has {kde.rect(kde.wid('tiled'))}")
want = asked()
checks.check(
    "its tile is as far from the edge as the file's workspace padding says",
    want is not None and want.x == 40,
)

print("== 3. systemd starts a komorebi that was killed again")
kde.systemctl("kill", "--signal=SIGKILL", UNIT)
checks.check(
    "the unit runs a new daemon",
    kde.wait_for(lambda: active() and main_pid() not in ("0", pid), timeout=30),
)
checks.check("... which answers komorebic", kde.wait_for(answering, timeout=60))
checks.check(
    "... and manages the window again",
    kde.wait_for(lambda: "tiled" in managed_titles(), timeout=30),
)

print("== 4. a stop is not a failure")
kde.systemctl("stop", UNIT)
checks.check("stopped with SIGTERM, the unit is inactive", not active())
checks.check("... and not failed", not failed())
kde.systemctl("start", UNIT)
checks.check("started again, its daemon answers", kde.wait_for(answering, timeout=60))
kde.komorebic("stop")
checks.check("komorebic stop stops the unit", kde.wait_for(lambda: not active(), timeout=30))
checks.check("... which is not failed", not failed())
checks.check("... and not started again", not kde.wait_for(active, timeout=5))

print("== 5. a daemon follows the file it is started with, and reloads it when it changes")
MINE = "/home/komorebi/mine.json"


def write(text: str) -> None:
    kde.sh(f"printf %s {shlex.quote(text)} > {MINE}")


def resize_delta() -> int:
    return json.loads(kde.komorebic("generate-static-config"))["resize_delta"]


def workspace_names() -> list[str]:
    elements = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"]
    return [workspace["name"] for workspace in elements]


write('{ "resize_delta": 100 }')
kde.komorebic("start", "--config", MINE)
checks.check("komorebic start --config starts a daemon that answers", kde.wait_for(answering))
checks.check("... which has the settings of the file", resize_delta() == 100)

write(
    '{ "resize_delta": 33, "monitors": [ { "workspaces": ['
    ' { "name": "one" }, { "name": "two" }, { "name": "three" } ] } ] }'
)
checks.check(
    "a change to the file is applied",
    kde.wait_for(
        lambda: resize_delta() == 33 and workspace_names() == ["one", "two", "three"],
        timeout=30,
    ),
)
kde.komorebic("focus-workspace", "2")
checks.check(
    "... and a workspace it added is one KWin shows",
    kde.wait_for(lambda: kde.shown() == 3, timeout=30),
)
kde.komorebic("stop")
checks.check("that daemon stops", kde.wait_for(lambda: not answering(), timeout=30))

checks.finish()
