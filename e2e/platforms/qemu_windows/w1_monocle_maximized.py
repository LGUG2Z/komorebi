"""W1: what toggle-monocle does while the user has a window maximized.

usage: python -m platforms.qemu_windows.w1_monocle_maximized GOLDEN BIN_DIR OUT STATE

Tiles three windows, has the user maximize the second, then sends
toggle-monocle twice and toggle-maximize, and prints what the daemon has
(the maximized window, containers, the monocle) after each. A probe.
"""

import importlib
import sys
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .windows_platform import WindowsPlatform


def main(argv: list[str]) -> int:
    golden, bin_dir, out, state = (Path(a) for a in argv[:4])
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        target = clients[1]
        area = platform.work_area()

        def describe() -> str:
            workspace = platform.state().workspace()
            window = platform.os_window(target)
            fills = window is not None and window.frame.w >= area.w - 4
            return (f"maximized "
                    f"{workspace.maximized.title if workspace.maximized else None}"
                    f", containers {len(workspace.containers)}, floating "
                    f"{len(workspace.floating)}, monocle "
                    f"{workspace.monocle is not None}, fills {fills}")

        platform.user_maximize(target)
        platform.wait_for(
            lambda: platform.state().workspace().maximized is not None, 10)
        print(f"PASTE after the user maximized: {describe()}")
        for command in ("toggle-monocle", "toggle-monocle", "toggle-maximize"):
            platform.komorebic(command)
            platform.wait_for(lambda: False, 3)
            print(f"PASTE after {command}: {describe()}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
