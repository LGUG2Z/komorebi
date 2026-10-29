"""Does kfw lose a window that is sent to a monitor that does not exist?

usage: python -m platforms.qemu_windows.w1_sendmon GOLDEN BIN_DIR OUT

One display, three tiled windows, then `komorebic send-to-monitor 1`; prints
the daemon's window count and the OS's before and after. A probe.
"""

import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import tiling

from .guest import Guest
from .windows_platform import WindowsPlatform


def counts(platform: WindowsPlatform, titles: list[str]) -> str:
    managed = platform.state().managed()
    visible = [t for t in titles if platform.os_window_by_title(t)]
    return f"daemon manages {managed}, OS has {len(visible)}: {visible}"


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv)
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, ["komorebi.exe", "komorebic.exe",
                               "komorebi-testwin.exe"])
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        titles = [c.title for c in clients]
        print("PASTE before:", counts(platform, titles))
        status, text = platform.komorebic_status("send-to-monitor", "1")
        print("PASTE send-to-monitor 1 ->", status, repr(text.strip()))
        time.sleep(3)
        print("PASTE after: ", counts(platform, titles))
        platform.screenshot("after")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
