r"""What only the Plasma shell tells about wallpapers, beside the shared `wallpaper` scenario, which
sets them, follows them from workspace to workspace with the borders, and sets one back: how the
shell fills the screen, a switch that komorebi did not make, the palette cache and a shell that
starts late.

  A  the fill mode of the wallpaper, and how long a workspace takes to change it
  B  a switch komorebi did not make (a pager's) sets the wallpaper too
  E  the palette cache
  F  komorebi that starts before the Plasma shell does

The pictures are a dozen bands of color each, since a palette wants at least eight: a warm one for
the monitor, a cool one for the second workspace, and a green one the test sets by hand.
"""

import re
import shlex
import time
from typing import Any

from komorebi_vm import Checks, Kde

UNIT = "komorebi.service"
SHELL_UNIT = "plasma-plasmashell.service"
# the daemon's log of the day, without colors, which every run of the unit adds to
LOG = "/home/komorebi/.local/share/komorebi/komorebi_plaintext.log.*"
CONFIG = "/home/komorebi/.config/komorebi/komorebi.json"
PICTURES = "/tmp/wallpapers"
WARM = f"{PICTURES}/warm.png"
COOL = f"{PICTURES}/my pictures/cool.png"
OTHER = f"{PICTURES}/other.png"
WARM_URL = f"file://{PICTURES}/warm.png"
COOL_URL = f"file://{PICTURES}/my%20pictures/cool.png"
# `Image.PreserveAspectCrop`, which is how komorebi has the shell fill the screen
FILL = 2


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `wallpaper`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)

    def running() -> bool:
        return kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0

    def answering() -> bool:
        return kde.run("komorebic monitor-information")[0] == 0

    def start_komorebi() -> None:
        kde.systemctl("start", UNIT)
        kde.wait_for(answering, timeout=60)
        time.sleep(2)

    def stop_komorebi() -> None:
        kde.systemctl("stop", UNIT)
        kde.close_colored_windows()

    def shell_wallpaper() -> tuple[str, int]:
        """What the shell shows on its first screen: the picture's URL, and the fill mode."""
        out = kde.run(
            "busctl --user call org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell wallpaper u 0"
        )[1]
        image = re.search(r'"Image" s "([^"]*)"', out)
        fill = re.search(r'"FillMode" i (\d+)', out)
        return (image.group(1) if image else "", int(fill.group(1)) if fill else -1)

    def shows(url: str) -> bool:
        return shell_wallpaper()[0] == url

    # the shared `wallpaper` scenario ran before this: the daemon runs with the wallpapers of its
    # configuration, the pictures are in PICTURES, and the first workspace shows the warm one
    # the shared scenario restarted the module's unit with the user's `komorebi.json`
    # the unit starts komorebi with the user's own `komorebi.json`: the wallpapers are in it too
    config = (
        '{ "monitors": [ { "wallpaper": { "path": "' + WARM + '" },'
        ' "workspaces": [ { "name": "one" },'
        ' { "name": "two", "wallpaper": { "path": "' + COOL + '" } } ] } ] }'
    )
    kde.sh(f"mkdir -p {shlex.quote(CONFIG.rsplit('/', 1)[0])}")
    kde.sh(f"echo {shlex.quote(config)} > {CONFIG}")

    print("== A. what only the Plasma shell tells")
    checks.check("the monitor's wallpaper fills the screen", shell_wallpaper()[1] == FILL)
    began = time.monotonic()
    kde.komorebic("focus-workspace", "1")
    checks.check(
        "workspace two's own wallpaper is shown when it is",
        kde.wait_for(lambda: shows(COOL_URL), timeout=10),
    )
    took = time.monotonic() - began
    checks.check(f"it took under two seconds ({took:.1f} s)", took < 2 * max(1, SCALE))
    kde.komorebic("focus-workspace", "0")
    kde.wait_for(lambda: shows(WARM_URL), timeout=10)

    print("== B. a switch somebody else made")
    kde.sh("kdotool set_desktop 2")
    checks.check(
        "the desktop was changed outside komorebi, and its wallpaper came with it",
        kde.wait_for(lambda: shows(COOL_URL), timeout=10),
    )
    kde.sh("kdotool set_desktop 1")
    checks.check("and back", kde.wait_for(lambda: shows(WARM_URL), timeout=10))

    print("== E. the palette is kept")
    cache = kde.sh("ls ~/.cache/komorebi 2>/dev/null || true").split()
    print(f"     the cache: {cache}")
    checks.check(
        "a palette for each wallpaper is in the cache, named for the picture",
        any(n.startswith("warm-") for n in cache) and any(n.startswith("cool-") for n in cache),
    )
    daemon_log = machine.execute(f"cat {LOG}")[1]
    checks.check(
        "komorebi did not complain",
        "failed to set wallpaper" not in daemon_log
        and "gave up setting the wallpaper" not in daemon_log,
    )
    stop_komorebi()

    print("== F. komorebi starts before the Plasma shell")
    kde.sh(f"plasma-apply-wallpaperimage {shlex.quote(OTHER)}")
    kde.systemctl("stop", SHELL_UNIT)
    checks.check(
        "the shell is gone",
        kde.wait_for(lambda: not kde.has_name("org.kde.plasmashell"), timeout=20),
    )
    start_komorebi()
    time.sleep(3)
    checks.check("no shell to answer: komorebi is still managing", answering())
    kde.systemctl("start", SHELL_UNIT)
    checks.check(
        "the shell is back", kde.wait_for(lambda: kde.has_name("org.kde.plasmashell"), timeout=60)
    )
    checks.check(
        "once the shell is up the wallpaper is set",
        kde.wait_for(lambda: shows(WARM_URL), timeout=60),
    )
    daemon_log = machine.execute(f"cat {LOG}")[1]
    checks.check(
        "and komorebi did not give up and say so", "gave up setting the wallpaper" not in daemon_log
    )
    stop_komorebi()
