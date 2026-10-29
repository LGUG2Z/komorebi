"""W1: can a window be moved while the screen is locked, and does kfw undo it?

usage: python -m platforms.qemu_windows.w1_lock GOLDEN BIN_DIR OUT [STATE]

Tiles the windows, locks, moves the first window to (400, 300) and prints its
frame and the daemon's idea of the managed windows at once, after 5 s locked,
and after the unlock. A probe: it prints what it saw.
"""

import importlib
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .windows_platform import WindowsPlatform


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    state = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        first = clients[0]
        print(f"PASTE tiled: {platform.os_window(first).frame}")
        platform.lock()
        print(f"PASTE locked; windows the OS lists: "
              f"{len(guest.agent.os_windows())}")
        platform.move_window(first, 400, 300)
        print(f"PASTE moved, at once: {platform.os_window(first).frame}")
        time.sleep(5)
        print(f"PASTE 5 s later, still locked: "
              f"{platform.os_window(first).frame}")
        platform.unlock()
        time.sleep(1)
        print(f"PASTE just unlocked: {platform.os_window(first).frame}")
        time.sleep(5)
        print(f"PASTE 5 s after the unlock: {platform.os_window(first).frame}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
