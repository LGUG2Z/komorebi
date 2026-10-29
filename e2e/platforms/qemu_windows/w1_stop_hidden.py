"""W1: what does `komorebic stop` do to windows that kfw has put away?

usage: python -m platforms.qemu_windows.w1_stop_hidden GOLDEN BIN_DIR OUT

Opens the tiling windows, sends one to workspace 1 (kfw cloaks it) and
minimizes another, stops the daemon, and prints for each window whether it
is cloaked or minimized and where it is; then starts the daemon again and
prints the same, with the workspace the daemon has each on. A probe: it
records what it saw, and the lifecycle scenario asks for none of this.
"""

import importlib
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .windows_platform import WindowsPlatform


def show(platform: WindowsPlatform, clients: list, label: str) -> None:
    print(f"PASTE {label}")
    for c in clients:
        w = platform.os_window_by_title(c.title)
        if w is None:
            print(f"PASTE   {c.title}: no window")
            continue
        print(f"PASTE   {c.title}: cloaked={w['cloaked']} "
              f"minimized={w.get('minimized')} frame={w['frame']}")


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        away, minimized = clients[-1], clients[0]

        platform.activate(away)
        platform.wait_for(lambda: True, 1)
        platform.komorebic("send-to-workspace", "1")
        platform.wait_for(lambda: platform.hidden(away))
        window = platform.os_window_by_title(minimized.title)
        guest.agent.call("minimize", hwnd=window["hwnd"])
        time.sleep(2)
        show(platform, clients, "before the stop (win1 minimized, "
             f"{away.title} on workspace 1)")

        platform.komorebic("stop")
        platform.wait_for(
            lambda: platform.komorebic_status("state")[0] != 0, 20
        )
        time.sleep(2)
        show(platform, clients, "after the stop")

        platform.start_komorebi()
        time.sleep(3)
        show(platform, clients, "after the start")
        state = platform.state()
        # workspace 1 is gone after a start without state, so ask for 0 only
        print(f"PASTE workspace 0 holds {state.containers(0)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
