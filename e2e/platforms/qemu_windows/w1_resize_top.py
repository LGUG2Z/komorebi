"""W1: does a drag that presses on the top row of a frame resize from the top edge?

usage: python -m platforms.qemu_windows.w1_resize_top GOLDEN BIN_DIR OUT [STATE] [INSIDE]

Runs `resize.edge_cases` with the press point of the top, topleft and topright
grabs moved from OFFSET px above the frame to INSIDE px (default 2) inside its
top row, the rest as they are, and prints the checks. A probe: the shared
scenario is not changed.
"""

import importlib
import sys
from pathlib import Path

from .guest import Guest
from .run import BINARIES, RecordedChecks
from .windows_platform import WindowsPlatform

TOP = ("top", "topleft", "topright")


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    state = Path(argv[3]) if len(argv) > 3 and argv[3] != "-" else None
    inside = int(argv[4]) if len(argv) > 4 else 2
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    resize = importlib.import_module("komorebi_vm.core.scenarios.resize")
    original = resize._press_point

    def press_point(grab, r):
        x, y = original(grab, r)
        if grab in TOP:
            y = r.y + inside
        return x, y

    resize._press_point = press_point
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        checks = RecordedChecks()
        clients = tiling.run(platform, checks)
        resize.edge_cases(platform, checks, clients)
        print(f"PASTE press {inside} px inside the top row: passed "
              f"{checks.passed} failed {checks.failed}")
        for failure in checks.failures:
            print(f"PASTE failed: {failure}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
