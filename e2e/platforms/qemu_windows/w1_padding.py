"""W1: does kfw's state agree with how far its windows sit from the edge?

usage: python -m platforms.qemu_windows.w1_padding GOLDEN BIN_DIR OUT [STATE]

Opens the tiling windows and prints the padding the state gives, the work
area, and each frame's distance from the work area's edges. A probe: it
prints what it saw.
"""

import importlib
import sys
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
        workspace = platform.state().workspace()
        print(f"PASTE state: workspace padding {workspace.workspace_padding}"
              f", container padding {workspace.container_padding}")
        area = platform.work_area()
        print(f"PASTE work area {area}")
        for c in clients:
            f = platform.os_window(c).frame
            print(f"PASTE {c.title}: frame {f}; from the left "
                  f"{f.x - area.x}, top {f.y - area.y}, right "
                  f"{area.x + area.w - f.x - f.w}, bottom "
                  f"{area.y + area.h - f.y - f.h}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
