# The NixOS module runs komorebi in a real Plasma session: nothing in this test starts the daemon.
# What only a real session can tell: that logging in starts the module's user unit through
# `plasma-workspace.target`, that the daemon it runs reaches KWin from there and tiles a window
# opened in the session, that it runs with the module's log level and the PATH of the user's
# session, that systemd starts it again when it is killed, and that a clean stop is not a failure.
# The options and the unit the module writes are checked without a machine by `nixos-module-eval`
# (`nix/tests/nixos-module.nix`).
import json

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
checks.check("... at the module's log level", command[1:] == ["--log-level", "debug"])
environment = kde.sh(f"tr '\\0' '\\n' < /proc/{pid}/environ").splitlines()
path = next((line[len("PATH=") :] for line in environment if line.startswith("PATH=")), "")
checks.check(
    "... with the PATH of the user's session",
    path.split(":")
    == ["/run/wrappers/bin", "/etc/profiles/per-user/komorebi/bin", "/run/current-system/sw/bin"],
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

checks.finish()
