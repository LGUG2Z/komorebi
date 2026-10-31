r"""The keys of the engine survive the daemon and KWin going away. kglobalaccel keeps the keys of a
client that died, swallowed and running nothing, until they are unregistered; and it runs inside
KWin, so a KWin that restarts forgets them. Only a real session can say that the module's unit
releases the keys of a daemon that was killed, that a new daemon takes them over, and that the
daemon registers them again with the kglobalaccel of a new KWin.
"""

import json
import shlex
from typing import Any

from komorebi_vm import Checks, Kde, Keys


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `restart-survival`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)
    keys = Keys(machine)

    def presses() -> int:
        """How many times a binding has run."""
        return len(machine.execute("cat /tmp/keys-restart")[1].split())

    def press(combo: str) -> bool:
        before = presses()
        keys.send(combo)
        return kde.wait_for(lambda: presses() > before, timeout=10)

    def registered(actions: list[str], timeout: float = 30) -> bool:
        return kde.wait_for(lambda: kde.shortcut_actions() == actions, timeout=timeout)

    def write(skhdrc: str) -> None:
        kde.sh(
            f"mkdir -p ~/.config/komorebi && printf '%s' {shlex.quote(skhdrc)}"
            " > ~/.config/komorebi/skhdrc"
        )

    def main_pid() -> str:
        return kde.systemctl("show", "--property=MainPID", "--value", "komorebi.service").strip()

    # the module's unit runs the daemon, which reads the skhdrc of the config directory at start
    write("alt - h : echo h >> /tmp/keys-restart\n")
    kde.systemctl("restart", "komorebi.service")
    checks.check("the daemon of the unit registers Alt+H", registered(["Alt+H"]))
    checks.check("Alt+H runs its binding", press("alt-h"))

    state = json.loads(kde.komorebic("hotkeys", "state"))
    checks.check(
        f"hotkeys state shows the key and the engine ({state})",
        state["registered"] == ["Alt+H"]
        and state["engine"]["mode"] == "default"
        and state["engine"]["active"] == ["Alt+H"],
    )

    # a daemon that does not answer leaves its keys dead, until they are released
    machine.succeed("pkill -STOP -x komorebi")
    before = presses()
    keys.send("alt-h")
    checks.check(
        "the key of a stopped daemon runs nothing",
        not kde.wait_for(lambda: presses() > before, timeout=3),
    )
    released = kde.komorebic("hotkeys", "release")
    checks.check(f"hotkeys release unregisters it ({released.strip()})", "Alt+H" in released)
    checks.check("no key is registered after the release", kde.shortcut_actions() == [])
    machine.succeed("pkill -CONT -x komorebi")
    kde.systemctl("restart", "komorebi.service")
    checks.check("a new daemon registers the key again", registered(["Alt+H"]))
    checks.check("Alt+H runs its binding again", press("alt-h"))

    # a daemon that is killed: the unit releases its keys, and systemd starts a new one
    old = main_pid()
    # the main process alone, as a crash: killing the whole unit would kill ExecStopPost too
    kde.systemctl("kill", "--kill-whom=main", "--signal=SIGKILL", "komorebi.service")
    checks.check(
        "systemd starts the daemon again",
        kde.wait_for(lambda: main_pid() not in ("0", old), timeout=30),
    )

    def journal() -> str:
        """The journal of the machine since it booted: the unit's lines, ExecStopPost's among them."""
        return machine.execute("journalctl --no-pager -o cat -b")[1]

    released = kde.wait_for(lambda: "released 1 keys: Alt+H" in journal(), timeout=10)
    words = ("released", "had no key", "could not release", "komorebi.service")
    seen = [line for line in journal().splitlines() if any(word in line for word in words)]
    checks.check(
        f"the unit released the keys of the killed daemon (it logged {seen[-6:]})", released
    )
    checks.check("the new daemon has the key", registered(["Alt+H"]))
    checks.check("Alt+H runs its binding after the kill", press("alt-h"))

    # KWin restarts, and kglobalaccel with it
    kwins = machine.succeed("pgrep -a kwin_wayland || true")
    print(kwins)
    compositors = [
        line.split()[0]
        for line in kwins.splitlines()
        if line.strip() and "wrapper" not in line.split()[1]
    ]
    machine.succeed(f"kill -KILL {' '.join(compositors)}")
    checks.check(
        "a new KWin owns kglobalaccel",
        kde.wait_for(lambda: kde.has_name("org.kde.kglobalaccel"), timeout=60),
    )
    checks.check(
        "the daemon registers the key with the new KWin", registered(["Alt+H"], timeout=60)
    )
    checks.check("Alt+H runs its binding after KWin restarted", press("alt-h"))

    # a reload reads the file again
    write("alt - j : echo j >> /tmp/keys-restart\n")
    kde.komorebic("hotkeys", "reload")
    checks.check("after a reload the daemon has the key of the new file", registered(["Alt+J"]))
    checks.check("Alt+J runs its binding", press("alt-j"))
