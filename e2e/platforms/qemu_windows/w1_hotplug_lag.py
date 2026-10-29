"""W1: how long does kfw take to drop a monitor that was unplugged?

usage: python -m platforms.qemu_windows.w1_hotplug_lag GOLDEN BIN_DIR OUT [N]

N times (default 5): plugs a display and, once kfw has a monitor for it,
unplugs it, then polls the OS and `komorebic state` every second for up to
90 s and prints when each says one display. With the daemon at debug level
the daemon's log of each round is kept in OUT. A probe: it prints what it saw.
"""

import sys
import time
from pathlib import Path

from .guest import Guest
from .run import BINARIES, _fetch_logs
from .windows_platform import WindowsPlatform


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    rounds = int(argv[3]) if len(argv) > 3 else 5
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.start_komorebi(log_level="debug")
        for n in range(1, rounds + 1):
            print(f"PASTE load {open('/proc/loadavg').read().split()[0]}")
            platform.add_display()
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                if len(platform.state().monitors) == 2:
                    break
                time.sleep(1)
            print(f"PASTE round {n}: kfw monitors "
                  f"{len(platform.state().monitors)} after the plug")
            t0 = time.monotonic()
            platform.remove_display()
            os_after = time.monotonic() - t0
            seen_os = seen_kfw = None
            while time.monotonic() - t0 < 90:
                if seen_os is None and len(platform.displays()) == 1:
                    seen_os = time.monotonic() - t0
                if len(platform.state().monitors) == 1:
                    seen_kfw = time.monotonic() - t0
                    break
                time.sleep(1)
            print(f"PASTE round {n}: remove_display returned after "
                  f"{os_after:.1f}s; OS one display at "
                  f"{seen_os if seen_os is None else round(seen_os, 1)}s; "
                  f"kfw one monitor at "
                  f"{seen_kfw if seen_kfw is None else round(seen_kfw, 1)}s")
        # the daemon's log of all the rounds
        _fetch_logs(guest, out)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
