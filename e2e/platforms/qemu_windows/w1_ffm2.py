"""W1: what does focus follows mouse need to work after it was turned off?

usage: python -m platforms.qemu_windows.w1_ffm2 GOLDEN BIN_DIR OUT [STATE]

The failing sequence of the `focus_follows_mouse` scenario, in a fresh guest
for each variant. With it on, the pointer goes over `a`, then `b`; it is
turned off with `toggle`, the pointer parked at PARK; then the setting is turned
on with HOW, the pointer goes over `a` and then over `b`, and the probe prints
how long `b` took to get the focus. PARK is `b` (the scenario), `a`, or the
corner of the display (off both windows); HOW is `enable` or `toggle`.
A probe: it prints what it saw.
"""

import importlib
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .w1_ffm import focused, middle
from .windows_platform import WindowsPlatform

VARIANTS = [
    ("b", "enable"),       # the scenario
    ("corner", "enable"),  # off both windows
    ("a", "enable"),       # on the window that has the focus
    ("b", "toggle"),       # turned on with the toggle
]
TOGGLE = ("toggle-focus-follows-mouse", "--implementation", "windows")


def wait(platform, target, seconds=15):
    t0 = time.monotonic()
    while time.monotonic() - t0 < seconds:
        if focused(platform, target):
            return round(time.monotonic() - t0, 2)
        time.sleep(0.25)
    return None


def one(golden, bin_dir, out, state, park, how) -> None:
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / f"guest-{park}-{how}", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        a, b = clients[0], clients[1]
        pa, pb = middle(platform, a), middle(platform, b)
        spots = {"a": pa, "b": pb, "corner": (5, 5)}
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        platform.pointer.move(*pa)
        wait(platform, a)
        platform.pointer.move(*pb)
        wait(platform, b)
        platform.pointer.move(*pa)
        wait(platform, a)
        platform.komorebic(*TOGGLE)
        time.sleep(1)
        platform.pointer.move(*spots[park])
        time.sleep(3)
        if how == "enable":
            platform.komorebic("focus-follows-mouse", "enable")
        else:
            platform.komorebic(*TOGGLE)
        time.sleep(1)
        platform.pointer.move(*pa)
        wait(platform, a, 3)
        platform.pointer.move(*pb)
        print(f"PASTE parked off at {park}, turned on with {how}: "
              f"b focused after {wait(platform, b)}s")


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    state = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    for park, how in VARIANTS:
        one(golden, bin_dir, out, state, park, how)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
