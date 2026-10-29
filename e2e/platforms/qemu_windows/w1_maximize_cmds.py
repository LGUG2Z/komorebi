"""W1: which command undoes a maximize that the user made?

usage: python -m platforms.qemu_windows.w1_maximize_cmds GOLDEN BIN_DIR OUT STATE

Tiles three windows, has the user maximize the second, and for each command
the maximizing scenario says the daemon ignores in that state: sends it,
waits, and prints what the daemon has (the maximized window, how many
containers, what floats, the monocle) and whether the window still fills the
work area; if the command undid the maximize, the user maximizes again. A
probe: it prints what it saw.
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
    maximizing = importlib.import_module("komorebi_vm.core.scenarios.maximizing")
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
            return (f"maximized {workspace.maximized.title if workspace.maximized else None}, "
                    f"containers {len(workspace.containers)}, floating "
                    f"{len(workspace.floating)}, monocle "
                    f"{workspace.monocle is not None}, fills {fills}")

        platform.user_maximize(target)
        platform.wait_for(lambda: platform.state().workspace().maximized is not None, 10)
        print(f"PASTE after the user maximized: {describe()}")
        for command in maximizing.IGNORED:
            status, text = platform.komorebic_status(*command.split())
            platform.wait_for(lambda: False, 2)
            print(f"PASTE {command!r} (exit {status}): {describe()}")
            if platform.state().workspace().maximized is None:
                # put it back, to look at the next command alone
                platform.komorebic("focus-workspace", "0")
                try:
                    platform.user_maximize(target, on=False)
                except RuntimeError:
                    pass
                platform.user_maximize(target)
                platform.wait_for(
                    lambda: platform.state().workspace().maximized is not None, 10)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
