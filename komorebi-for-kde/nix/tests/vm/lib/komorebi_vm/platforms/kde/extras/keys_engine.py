r"""The engine in a real Plasma session: the daemon registers the keys of a key binding file with
KWin's global shortcut daemon and runs their commands. The Rust tests check the calls the
engine's effects become; only KWin can say that a grabbed key reports its presses to the daemon,
that the keys of a killed daemon stay registered until the next start takes them over, and that
a stop gives them back.
"""

import json
import shlex
from typing import Any

from komorebi_vm import Checks, Kde, Keys


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `keys-engine`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)
    keys = Keys(machine)

    def ran(marker: str) -> bool:
        return machine.execute(f"test -e /tmp/{marker}")[0] == 0

    def press(combo: str, marker: str) -> bool:
        keys.send(combo)
        return kde.wait_for(lambda: ran(marker), timeout=10)

    def engine() -> dict[str, Any]:
        """What the engine is doing: its `mode`, the keys it holds (`active`) and whether a chord
        `chain` is being typed. It is empty when no engine answers.

        The engine answers once it has told kglobalaccel everything its last event needed, so what it
        says it holds is what KWin has grabbed. A key pressed before that is not the engine's yet and
        goes to the window, and how long the grab takes is how slow the machine is: the engine is
        asked, and no fixed time is waited.
        """
        status, out = kde.run("komorebic hotkeys state")
        return (json.loads(out).get("engine") or {}) if status == 0 else {}

    def in_mode(mode: str) -> bool:
        """Waits for the engine to be in `mode`, with the keys of the mode grabbed."""
        return kde.wait_for(lambda: engine().get("mode") == mode, timeout=10)

    def chain_is(on: bool, within: float = 10) -> bool:
        """Waits for the engine to be in a chord chain (or, with `on` false, to be done with it, and to
        hold the keys of the mode again), for up to `within` seconds."""
        return kde.wait_for(lambda: engine().get("chain") is on, timeout=within)

    def write(skhdrc: str) -> None:
        kde.sh(
            f"mkdir -p ~/.config/komorebi && printf '%s' {shlex.quote(skhdrc)} > ~/.config/komorebi/skhdrc"
        )

    def start(skhdrc: str, *args: str) -> None:
        write(skhdrc)
        # no --keybindings: the daemon finds the skhdrc in the config directory
        kde.spawn_komorebi("--log-level", "debug", *args)
        if not kde.wait_for(lambda: "keys in kglobalaccel" in kde.komorebi_log(), timeout=60):
            raise AssertionError(f"the keys were not registered:\n{kde.komorebi_log()}")

    def stop() -> None:
        """Stops the daemon, and waits until it is gone: it takes its KWin script and its keys with
        it when it exits, and a daemon started before that would find them taken from it."""
        kde.komorebic("stop")
        if not kde.wait_for(lambda: machine.execute("pgrep -x komorebi")[0] != 0, timeout=10):
            raise AssertionError("the daemon did not stop")

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
    checks.check(
        f"the next start leaves only its own keys ({actions})", actions == ["Alt+H", "Alt+J"]
    )
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

    def activate(wid: str) -> None:
        """Gives the window the focus, asking again until KWin has: it lists a window before it
        can have the focus, and refuses to activate it until then."""
        if not kde.wait_for(
            lambda: (
                kde.run(f"kdotool windowactivate {shlex.quote(wid)}")[0] == 0
                and kde.active() == wid
            ),
            timeout=30,
        ):
            raise AssertionError(f"{wid} did not take the focus")

    activate(kde.wid_by_class("wev"))

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
        "the daemon logs the mode it is in, and holds the keys of the mode",
        kde.wait_for(lambda: "the mode is now resize" in kde.komorebi_log()) and in_mode("resize"),
    )
    checks.check("in the mode H runs its binding", press("h", "keys-modes-h"))
    checks.check("and does not reach the window", typed("h") == 1)
    keys.send("esc")
    checks.check(
        "Escape goes back to the default mode, and gives the keys of the mode back",
        kde.wait_for(lambda: "the mode is now default" in kde.komorebi_log())
        and in_mode("default"),
    )
    keys.send("h")
    checks.check("back in the default mode H reaches the window again", reaches_wev("h", 2))
    stop()

    # chord chains: the prefix grabs the keys that can come next, until a key, Escape or the timeout
    sxhkdrc = "/home/komorebi/.config/komorebi/sxhkdrc"
    chain = "alt + w ; {h,l}\n    touch /tmp/keys-chords-{h,l}\n"
    kde.sh(f"printf '%s' {shlex.quote(chain)} > {sxhkdrc}")
    # The timeout is long, and is scaled like every wait, so that the keys of a chain are sent well
    # inside it however slowly the machine grabs them, and that Escape can be told from the timeout.
    CHORD_TIMEOUT = 3
    kde.spawn_komorebi(
        "--log-level",
        "debug",
        "--keybindings",
        sxhkdrc,
        "--chord-timeout",
        str(int(CHORD_TIMEOUT * 1000 * SCALE)),
    )
    if not kde.wait_for(lambda: "keys in kglobalaccel" in kde.komorebi_log(), timeout=60):
        raise AssertionError(f"the keys were not registered:\n{kde.komorebi_log()}")

    def chain_begins() -> None:
        """Presses Alt+W and waits until the engine has grabbed the keys that can come next.

        A key pressed before that goes to the window, not to the chain.
        """
        keys.send("alt-w")
        if not chain_is(True):
            raise AssertionError(
                f"Alt+W did not begin the chain ({engine()}):\n{kde.komorebi_log()}"
            )

    def chain_ends() -> None:
        """Waits until the chain is over and the engine holds Alt+W again, which a key pressed
        before that would not find."""
        if not chain_is(False):
            raise AssertionError(f"the chain did not end ({engine()}):\n{kde.komorebi_log()}")

    h_before = typed("h")
    chain_begins()
    checks.check("Alt+W, H runs the binding of the chain", press("h", "keys-chords-h"))
    checks.check("the H of the chain does not reach the window", typed("h") == h_before)
    chain_ends()
    chain_begins()
    keys.send("esc")
    checks.check(
        "Escape ends the chain, which the timeout has not yet done",
        chain_is(False, within=CHORD_TIMEOUT / 2),
    )
    keys.send("h")
    checks.check("after Alt+W, Escape, H reaches the window", reaches_wev("h", h_before + 1))
    chain_begins()
    checks.check("the chain ends at its timeout", chain_is(False, within=CHORD_TIMEOUT + 1))
    keys.send("l")
    checks.check("after Alt+W and the timeout, L reaches the window", reaches_wev("l", 1))
    checks.check("and runs nothing", not ran("keys-chords-l"))
    stop()

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

    def sees_focus(wid: str, app_id: str) -> bool:
        """Activates the window and waits for the engine to log that its application has the focus."""
        activate(wid)

        def seen() -> bool:
            lines = [x for x in kde.komorebi_log().splitlines() if "the focus is now" in x]
            return bool(lines) and f"({app_id}, " in lines[-1]

        return kde.wait_for(seen, timeout=10)

    def focus(wid: str, app_id: str) -> None:
        """Activates the window, and returns once the engine has grabbed the keys that its
        application has an action for, and given the others back."""
        if not sees_focus(wid, app_id):
            raise AssertionError(f"the engine did not see {app_id} take the focus")
        # the engine logs the focus before it grabs and gives back the keys of the application, and
        # answers once it has: a key pressed before that would find the keys of the other window
        if not engine():
            raise AssertionError("the engine does not answer")

    focus(wev, "wev")
    g_before, n_before = typed("g"), typed("n")
    keys.send("alt-g")
    checks.check(
        "in wev, Alt+G (for the terminal only) reaches wev", reaches_wev("g", g_before + 1)
    )
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

    # An engine that starts while the focused application has nothing to grab starts all the same:
    # the key bindings are read again with wev focused, and every binding is for the terminal.
    write('alt - j [\n    "terminal" : touch /tmp/keys-scopes-reload\n]\n')
    reads = kde.komorebi_log().count("bindings for the engine")
    kde.komorebic("hotkeys", "reload")
    kde.wait_for(lambda: kde.komorebi_log().count("bindings for the engine") > reads, timeout=10)
    checks.check(
        "an engine that starts with nothing to grab goes on to see foot take the focus",
        sees_focus(foot, "foot") and bool(engine()),
    )
    checks.check(
        "and in foot, Alt+J runs the terminal's binding",
        press("alt-j", "keys-scopes-reload"),
    )
    stop()
