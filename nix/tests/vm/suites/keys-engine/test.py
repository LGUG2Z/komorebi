# The engine in a real Plasma session: the daemon registers the keys of a key binding file with
# KWin's global shortcut daemon and runs their commands. The Rust tests check the calls the
# engine's effects become; only KWin can say that a grabbed key reports its presses to the daemon,
# that the keys of a killed daemon stay registered until the next start takes them over, and that
# a stop gives them back.
import shlex
import time

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


def start(skhdrc: str, *args: str) -> None:
    kde.sh(
        f"mkdir -p ~/.config/komorebi && printf '%s' {shlex.quote(skhdrc)} > ~/.config/komorebi/skhdrc"
    )
    # no --keybindings: the daemon finds the skhdrc in the config directory
    kde.spawn_komorebi("--log-level", "debug", *args)
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

# modes: entering one grabs its keys, a bare H among them, and lets the others go to the window
kde.spawn("stdbuf -oL wev", log="/tmp/wev.log")
if not kde.wait_for(lambda: kde.wid_by_class("wev") != "", timeout=30):
    raise AssertionError("wev did not open")
kde.activate(kde.wid_by_class("wev"))


def typed(key: str) -> int:
    """How many presses of `key` reached wev."""
    # wev prints a sym line after the press and after the release of a key
    presses = f"grep -A1 'state: 1 (pressed)' /tmp/wev.log | grep -cE 'sym: {key} +\\('"
    out = machine.execute(presses)[1].strip()
    return int(out or "0")


def reaches_wev(key: str, presses: int) -> bool:
    return kde.wait_for(lambda: typed(key) >= presses, timeout=5)


start(
    ":: resize : touch /tmp/keys-modes-entered\n"
    "alt - r ; resize\n"
    "resize < h : touch /tmp/keys-modes-h\n"
    "resize < escape ; default\n"
)
keys.send("h")
checks.check("in the default mode H reaches the focused window", reaches_wev("h", 1))
checks.check("Alt+R enters the mode and runs its command", press("alt-r", "keys-modes-entered"))
checks.check(
    "the daemon logs the mode it is in",
    kde.wait_for(lambda: "the mode is now resize" in kde.komorebi_log()),
)
checks.check("in the mode H runs its binding", press("h", "keys-modes-h"))
checks.check("and does not reach the window", typed("h") == 1)
keys.send("esc")
checks.check(
    "Escape goes back to the default mode",
    kde.wait_for(lambda: "the mode is now default" in kde.komorebi_log()),
)
keys.send("h")
checks.check("back in the default mode H reaches the window again", reaches_wev("h", 2))
kde.komorebic("stop")

# chord chains: the prefix grabs the keys that can come next, until a key, Escape or the timeout
sxhkdrc = "/home/komorebi/.config/komorebi/sxhkdrc"
chain = "alt + w ; {h,l}\n    touch /tmp/keys-chords-{h,l}\n"
kde.sh(f"printf '%s' {shlex.quote(chain)} > {sxhkdrc}")
kde.spawn_komorebi("--log-level", "debug", "--keybindings", sxhkdrc, "--chord-timeout", "1500")
if not kde.wait_for(lambda: "keys in kglobalaccel" in kde.komorebi_log(), timeout=60):
    raise AssertionError(f"the keys were not registered:\n{kde.komorebi_log()}")
h_before = typed("h")
keys.send("alt-w")
checks.check("Alt+W, H runs the binding of the chain", press("h", "keys-chords-h"))
checks.check("the H of the chain does not reach the window", typed("h") == h_before)
keys.send("alt-w")
keys.send("esc")
keys.send("h")
checks.check("after Alt+W, Escape, H reaches the window", reaches_wev("h", h_before + 1))
keys.send("alt-w")
time.sleep(2 * SCALE)
keys.send("l")
checks.check("after Alt+W and the timeout, L reaches the window", reaches_wev("l", 1))
checks.check("and runs nothing", not ran("keys-chords-l"))
kde.komorebic("stop")

# per-application bindings: a key is grabbed only while the focused application has an action
start(
    'alt - g [\n    "terminal" : touch /tmp/keys-scopes-terminal\n]\n'
    'alt - n [\n    "wev" ~\n    * : touch /tmp/keys-scopes-other\n]\n',
    "--app-alias",
    "terminal=foot",
)
kde.spawn("foot")
if not kde.wait_for(lambda: kde.wid_by_class("foot") != "", timeout=30):
    raise AssertionError("foot did not open")
wev, foot = kde.wid_by_class("wev"), kde.wid_by_class("foot")


def focus(wid: str, app_id: str) -> None:
    """Activates the window and waits for the engine to see its application take the focus."""
    kde.activate(wid)

    def seen() -> bool:
        lines = [x for x in kde.komorebi_log().splitlines() if "the focus is now" in x]
        return bool(lines) and f"({app_id}, " in lines[-1]

    if not kde.wait_for(seen, timeout=10):
        raise AssertionError(f"the engine did not see {app_id} take the focus")


focus(wev, "wev")
g_before, n_before = typed("g"), typed("n")
keys.send("alt-g")
checks.check("in wev, Alt+G (for the terminal only) reaches wev", reaches_wev("g", g_before + 1))
keys.send("alt-n")
checks.check("in wev, Alt+N (which wev ignores) reaches wev", reaches_wev("n", n_before + 1))
checks.check("and runs nothing", not ran("keys-scopes-other"))
focus(foot, "foot")
checks.check(
    "in foot, Alt+G runs the terminal's binding, by the alias",
    press("alt-g", "keys-scopes-terminal"),
)
checks.check(
    "in foot, Alt+N runs the binding for every other application",
    press("alt-n", "keys-scopes-other"),
)
focus(wev, "wev")
keys.send("alt-g")
checks.check("back in wev, Alt+G reaches wev again", reaches_wev("g", g_before + 2))
kde.komorebic("stop")

checks.finish()
