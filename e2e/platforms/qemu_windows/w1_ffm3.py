"""W1: which step of the focus_follows_mouse scenario makes its last check fail?

usage: python -m platforms.qemu_windows.w1_ffm3 GOLDEN BIN_DIR OUT STATE

The scenario's steps, in a fresh guest for each variant, with the pieces that
differ from `w1_ffm2` switched on or off: STATE (a `komorebic state` before
the toggle) and POLL (the scenario's 3 s poll of the second window while the
setting is off, instead of a sleep). Prints whether the second window got the
focus after the setting was turned on again. A probe.
"""

import importlib
import itertools
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .w1_ffm import focused, middle
from .w1_ffm2 import TOGGLE, wait
from .windows_platform import WindowsPlatform


def one(golden, bin_dir, out, state, with_state, poll) -> None:
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    name = f"state{int(with_state)}-poll{int(poll)}"
    with Guest(golden, out / name, state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        a, b = tiling.run(platform, Checks())[:2]
        pa, pb = middle(platform, a), middle(platform, b)
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        for target, point in ((a, pa), (b, pb), (a, pa)):
            platform.pointer.move(*point)
            wait(platform, target, 10)
        if with_state:
            platform.state().focused_title()
        platform.komorebic(*TOGGLE)
        time.sleep(1)
        platform.pointer.move(*pb)
        if poll:
            platform.wait_for(lambda: focused(platform, b), timeout=3)
        else:
            time.sleep(4.5)
        platform.komorebic("focus-follows-mouse", "enable")
        time.sleep(1)
        platform.pointer.move(*pa)
        wait(platform, a, 10)
        platform.pointer.move(*pb)
        print(f"PASTE {name}: b focused after {wait(platform, b)}s")


def main(argv: list[str]) -> int:
    golden, bin_dir, out, state = (Path(a) for a in argv[:4])
    out.mkdir(parents=True, exist_ok=True)
    for with_state, poll in itertools.product((False, True), repeat=2):
        one(golden, bin_dir, out, state, with_state, poll)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
