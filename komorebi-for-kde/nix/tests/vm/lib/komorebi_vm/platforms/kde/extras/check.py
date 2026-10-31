r"""`komorebic check` against a real (headless) KWin: what it finds of the desktop, and what it says
when something is missing: no Plasma shell for the wallpapers, a stock effect shadowing komorebi's
own, komorebi without its script in KWin, komorebi not running, no KWin on the bus. What only a
real KWin can tell: that its version is read from what KWin says of itself, that the effects are
komorebi's builds because of what they contain, that a daemon is told from its script by KWin's
own answer, that a bus with no KWin on it is an error with an exit status that says so, and that
the shortcut files the check compares with are in the directory `komorebic hotkeys compile`
writes to in a session. The configuration and the exit status are covered by
`komorebi/src/check.rs` and `komorebic/tests/check.rs`, against no desktop.
"""

import base64
import json
import re
import shlex
import struct
import zlib
from typing import Any

from komorebi_vm import Checks, Kde


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `check`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.headless(machine, "main", size=(1920, 1080), scale=SCALE)

    CONFIG = "/tmp/check-config.json"
    EMPTY_BUS = "/tmp/check-empty.bus"
    FLAT = "/tmp/check-flat.png"

    def flat_png() -> bytes:
        """A picture of one color all over: a wallpaper no theme can be made from."""
        raw = b"".join(b"\x00" + bytes([200, 40, 40]) * 8 for _ in range(8))

        def chunk(kind: bytes, data: bytes) -> bytes:
            body = kind + data
            return (
                struct.pack(">I", len(data))
                + body
                + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)
            )

        header = struct.pack(">IIBBBBB", 8, 8, 8, 2, 0, 0, 0)
        return (
            b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b"")
        )

    kde.sh(f"echo {base64.b64encode(flat_png()).decode()} | base64 -d > {FLAT}")
    # borders and animation, which want their effects; a wallpaper, which wants the Plasma shell and
    # cannot make a theme; one workspace that is a layout and one that is only a name, which komorebi
    # does not tile
    kde.sh(
        "printf %s "
        + shlex.quote(
            json.dumps(
                {
                    "border": True,
                    "animation": {"enabled": True, "style": "Linear"},
                    "monitors": [
                        {
                            "wallpaper": {"path": FLAT},
                            "workspaces": [{"name": "a", "layout": "BSP"}, {"name": "b"}],
                        }
                    ],
                }
            )
        )
        + f" > {CONFIG}"
    )

    def check(bus: str | None = None, data_home: str | None = None) -> tuple[int, str]:
        """Runs `komorebic check` on the session's bus, or on the bus at `bus`, with `data_home` as the
        user's data directory when KWin's environment does not name one."""
        prefix = f"DBUS_SESSION_BUS_ADDRESS=unix:path={bus} " if bus else ""
        prefix += f"XDG_DATA_HOME={data_home} " if data_home else ""
        status, out = kde.run(f"{prefix}komorebic check --komorebi-config {CONFIG}")
        print(out)
        return status, out

    print("== 1. a desktop that is as it should be, with komorebi running")
    kde.start_komorebi("--config", CONFIG)
    checks.check(
        "the daemon answers komorebic",
        kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=30),
    )
    checks.check("its script is in KWin", kde.wait_for(kde.script_loaded, timeout=30))
    status, out = check()
    checks.check("it passes: no errors", status == 0 and "\n0 error(s)" in out)
    checks.check(
        "it knows this is a KDE Plasma session on Wayland",
        "this is a KDE Plasma session" in out and "it is a Wayland session" in out,
    )
    checks.check("it knows KWin 6", re.search(r"ok +KWin 6\.", out) is not None)
    checks.check(
        "it sees komorebi, and its script in KWin",
        "komorebi is running, and its script is in KWin" in out,
    )
    checks.check(
        "it says what starts komorebi at login",
        re.search(r"ok +komorebi is started at login by ", out) is not None,
    )
    checks.check(
        "the border effect is komorebi's build",
        "the border effect is komorebi's build" in out,
    )
    checks.check(
        "the animation effect is komorebi's build",
        "the animation effect is komorebi's build" in out,
    )
    checks.check(
        "the configuration is read, and the workspace that is only a name is found",
        'workspace 1 of monitor 0 ("b") has no `layout`' in out,
    )
    checks.check(
        "wallpapers need the Plasma shell, which a headless KWin does not have",
        "the Plasma shell is not on the bus" in out,
    )
    checks.check(
        "a picture of one color cannot make a theme",
        f"no theme can be made from {FLAT}" in out,
    )

    print("== 2. the stock animation effect in the user's own directory hides komorebi's")
    DATA = "/tmp/check-data"
    STOCK = f"{DATA}/kwin/effects/kwin4_effect_geometry_change/contents/code"
    kde.sh(f"mkdir -p {STOCK} && echo 'curve: QEasingCurve.OutExpo' > {STOCK}/main.js")
    status, out = check(data_home=DATA)
    checks.check(
        "the effect is the stock one, and the style is ignored",
        'the animation effect is the stock one: `animation.style` "Linear" is ignored' in out,
    )
    checks.check("and it says which copy hides which", "shadows" in out)
    kde.sh(f"rm -rf {DATA}")

    print("== 3. komorebi is running and its script is not in KWin")
    kde.sh(
        "busctl --user call org.kde.KWin /Scripting org.kde.kwin.Scripting unloadScript s komorebi-bridge"
    )
    checks.check("KWin has let the script go", kde.wait_for(lambda: not kde.script_loaded()))
    status, out = check()
    checks.check(
        "it is an error, and the exit status says so",
        status == 1
        and re.search(r"error +komorebi is running but its script is not in KWin", out) is not None,
    )

    print("== 4. komorebi is not running")
    kde.stop_komorebi()
    checks.check("the daemon is gone", kde.wait_for(lambda: not kde.komorebi_running(), timeout=30))
    status, out = check()
    checks.check(
        "a note, not an error",
        status == 0 and re.search(r"note +komorebi is not running", out) is not None,
    )

    print("== 5. there is no KWin on the bus")
    pid = kde.sh(
        f"dbus-daemon --session --address=unix:path={EMPTY_BUS} --fork --print-pid"
    ).strip()
    status, out = check(EMPTY_BUS)
    kde.run(f"kill {pid}")
    checks.check(
        "KWin missing is an error, and the desktop is looked at no further",
        status == 1
        and "KWin is not on the session bus" in out
        and "komorebi is running" not in out
        and "komorebi is not running" not in out,
    )

    print("== 6. the key bindings")
    KEYS_HOME = "/tmp/check-keys-home"
    SHORTCUTS = "~/.local/share/kglobalaccel"
    STALE = "[Desktop Entry]\nX-Komorebi-Generated=true\n"

    def check_keys() -> tuple[int, str]:
        """Runs `komorebic check` on the key bindings alone, in a config home of their own."""
        status, out = kde.run(
            f"KOMOREBI_CONFIG_HOME={KEYS_HOME} komorebic check --configuration-only"
        )
        print(out)
        return status, out

    kde.sh(f"mkdir -p {KEYS_HOME}")
    status, out = check_keys()
    checks.check(
        "without a key binding file it is a note, not an error",
        status == 0
        and re.search(r"note +there is no skhdrc, whkdrc or sxhkdrc in ", out) is not None,
    )

    kde.sh(
        "printf %s "
        + shlex.quote("alt - h : komorebic focus left\nalt - nosuchkey : komorebic focus up\n")
        + f" > {KEYS_HOME}/skhdrc"
    )
    status, out = check_keys()
    checks.check(
        "the file is read, and the line it cannot read is a warning that does not fail the check",
        status == 0
        and "skhdrc has 1 key binding" in out
        and re.search(r"warning +1 line of it cannot be read and left out", out) is not None,
    )

    # a shortcut file komorebi wrote that the file does not call for, in the directory of the session
    kde.sh(
        f"mkdir -p {SHORTCUTS} && printf %s {shlex.quote(STALE)} > {SHORTCUTS}/komorebi-stale.desktop"
    )
    status, out = check_keys()
    checks.check(
        "it says the shortcut files are not the ones `hotkeys compile` would write",
        status == 0
        and "the shortcut files are not the ones `komorebic hotkeys compile` would write" in out,
    )
    kde.sh(f"KOMOREBI_CONFIG_HOME={KEYS_HOME} komorebic hotkeys compile")
    status, out = check_keys()
    checks.check(
        "after `hotkeys compile` it does not: both use the directory of the session",
        status == 0 and "are not the ones" not in out,
    )

    kde.stop()
