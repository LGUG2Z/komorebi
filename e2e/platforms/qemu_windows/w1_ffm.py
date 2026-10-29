"""W1: how soon does focus follows mouse (the windows implementation) act?

usage: python -m platforms.qemu_windows.w1_ffm GOLDEN BIN_DIR OUT

Opens the tiling windows, enables focus follows mouse, moves the pointer
between two windows and prints how long each takes to get the focus; then
turns it off with `toggle`, on with `enable`, and does the same, once with
a single pointer move and once with a few small ones. A probe: it prints
what it saw.
"""

import importlib
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .windows_platform import WindowsPlatform


def middle(platform, client):
    f = platform.os_window(client).frame
    return f.x + f.w // 2, f.y + f.h // 2


def focused(platform, client) -> bool:
    w = platform.os_window(client)
    return w is not None and w.focused


def timed(platform, label, target, point, wiggle=False) -> None:
    t0 = time.monotonic()
    platform.pointer.move(*point)
    seen = None
    while time.monotonic() - t0 < 15:
        if focused(platform, target):
            seen = time.monotonic() - t0
            break
        if wiggle:
            platform.pointer.move(point[0] + 3, point[1] + 3)
            platform.pointer.move(*point)
        time.sleep(0.25)
    print(f"PASTE {label}: focused after "
          f"{seen if seen is None else round(seen, 2)}s")


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.start_komorebi(log_level="debug")
        clients = tiling.run(platform, Checks())
        a, b = clients[0], clients[1]
        pa, pb = middle(platform, a), middle(platform, b)
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        timed(platform, "first enable, over a", a, pa)
        timed(platform, "first enable, over b", b, pb)
        timed(platform, "first enable, back over a", a, pa)
        platform.komorebic("toggle-focus-follows-mouse",
                           "--implementation", "windows")
        time.sleep(1)
        timed(platform, "toggled off, over b (expect never)", b, pb)
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        timed(platform, "enabled again, over a", a, pa)
        timed(platform, "enabled again, over b", b, pb)
        timed(platform, "enabled again, back over a, wiggling", a, pa, True)
        timed(platform, "enabled again, over b, wiggling", b, pb, True)
        # variant 1: disable and enable, no toggle
        platform.komorebic("focus-follows-mouse", "disable")
        time.sleep(1)
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        timed(platform, "disable+enable, over a", a, pa)
        timed(platform, "disable+enable, over b", b, pb)
        # variant 2: toggle twice
        platform.komorebic("toggle-focus-follows-mouse",
                           "--implementation", "windows")
        time.sleep(1)
        platform.komorebic("toggle-focus-follows-mouse",
                           "--implementation", "windows")
        time.sleep(1)
        timed(platform, "toggle twice, over a", a, pa)
        timed(platform, "toggle twice, over b", b, pb)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
