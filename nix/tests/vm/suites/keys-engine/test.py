# The engine in a real Plasma session: the daemon registers the keys of a key binding file with
# KWin's global shortcut daemon and runs their commands. The Rust tests check the calls the
# engine's effects become; only KWin can say that a grabbed key reports its presses to the daemon,
# that the keys of a killed daemon stay registered until the next start takes them over, and that
# a stop gives them back.
import shlex

from komorebi_vm import Checks, Kde, Keys

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)
keys = Keys(machine)


def ran(marker: str) -> bool:
    return machine.execute(f"test -e /tmp/{marker}")[0] == 0


def press(combo: str, marker: str) -> bool:
    keys.send(combo)
    return kde.wait_for(lambda: ran(marker), timeout=10)


def start(skhdrc: str) -> None:
    kde.sh(
        f"mkdir -p ~/.config/komorebi && printf '%s' {shlex.quote(skhdrc)} > ~/.config/komorebi/skhdrc"
    )
    # no --keybindings: the daemon finds the skhdrc in the config directory
    kde.spawn_komorebi("--log-level", "debug")
    if not kde.wait_for(lambda: "keys in kglobalaccel" in kde.komorebi_log(), timeout=60):
        raise AssertionError(f"the keys were not registered:\n{kde.komorebi_log()}")


start(
    "alt - h : touch /tmp/keys-engine-alt-h\n"
    "alt - r : komorebic retile\n"
    'alt - q : echo "$HOME" > /tmp/keys-engine-alt-q\n'
)
actions = kde.shortcut_actions()
checks.check(
    f"the daemon registered its three keys ({actions})", actions == ["Alt+H", "Alt+Q", "Alt+R"]
)
checks.check("Alt+H runs its command", press("alt-h", "keys-engine-alt-h"))
checks.check("Alt+Q runs its command through a shell", press("alt-q", "keys-engine-alt-q"))
home = machine.succeed("cat /tmp/keys-engine-alt-q").strip()
checks.check(f"the shell expanded $HOME ({home!r})", home == "/home/komorebi")
keys.send("alt-r")
checks.check(
    "Alt+R runs komorebic, which sends its message to the daemon",
    kde.wait_for(lambda: "processing command: Retile" in kde.komorebi_log(), timeout=10),
)

# kglobalaccel keeps the keys of a daemon that was killed
kde.kill_spawned_komorebi()
checks.check("a killed daemon leaves its keys registered", kde.shortcut_actions() == actions)

# the next start takes over the keys it binds, and unregisters the others
start("alt - j : touch /tmp/keys-engine-alt-j\nalt - h : touch /tmp/keys-engine-alt-h-again\n")
actions = kde.shortcut_actions()
checks.check(f"the next start leaves only its own keys ({actions})", actions == ["Alt+H", "Alt+J"])
checks.check("Alt+J runs its command", press("alt-j", "keys-engine-alt-j"))
checks.check("Alt+H runs its new command", press("alt-h", "keys-engine-alt-h-again"))

# a stop gives every key back
kde.komorebic("stop")
checks.check(
    "komorebic stop stops the daemon",
    kde.wait_for(lambda: machine.execute("pgrep -x komorebi")[0] != 0, timeout=10),
)
checks.check("a stop unregisters every key", kde.shortcut_actions() == [])
machine.succeed("rm /tmp/keys-engine-alt-j")
keys.send("alt-j")
checks.check(
    "Alt+J runs nothing once the daemon stopped",
    not kde.wait_for(lambda: ran("keys-engine-alt-j"), timeout=3),
)

checks.finish()
