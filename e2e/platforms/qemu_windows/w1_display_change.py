"""W1: what does kfw do when the resolution of a display changes?

usage: python -m platforms.qemu_windows.w1_display_change GOLDEN BIN_DIR OUT [STATE]

Plugs in a second display, sends two windows to it, changes its mode (through
display.ps1) to a larger and then a smaller one, and back, and every second
for 20 s prints the display's size as the OS has it, the daemon's monitor size
and work area, and whether the windows are inside the work area and clear of
each other. A probe: it prints what it saw.
"""

import importlib
import sys
import time
from pathlib import Path

from komorebi_vm.core.checks import Checks

from .guest import Guest
from .run import BINARIES
from .windows_platform import WindowsPlatform

HERE = Path(__file__).parent
SCRIPT = "C:\\e2e\\display.ps1"
DEVICE = "\\\\.\\DISPLAY2"
SIZES = [(1280, 720), (800, 600), (1024, 768)]


def display(guest: Guest, *args: str) -> str:
    result = guest.agent.exec(
        "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
        "-File", SCRIPT, *args,
    )
    return (result["stdout"] + result["stderr"]).strip()


def watch(platform: WindowsPlatform, moved, label: str, seconds: int = 20) -> None:
    t0 = time.monotonic()
    last = None
    while time.monotonic() - t0 < seconds:
        monitors = platform.state().monitors
        displays = platform.displays()
        if len(monitors) > 1 and len(displays) > 1:
            monitor, shown = monitors[1], displays[1]
            frames = [w.frame for c in moved
                      if (w := platform.os_window(c)) is not None]
            inside = all(f.inside(monitor.work_area, 1) for f in frames)
            apart = not any(a.overlaps(b) for i, a in enumerate(frames)
                            for b in frames[i + 1:])
            now = (f"OS {shown.w}x{shown.h}, daemon monitor "
                   f"{monitor.size.w}x{monitor.size.h}, work area "
                   f"{monitor.work_area.w}x{monitor.work_area.h}, windows "
                   f"inside {inside}, apart {apart}")
        else:
            now = f"monitors {len(monitors)}, displays {len(displays)}"
        if now != last:
            print(f"PASTE {label} +{time.monotonic() - t0:.0f}s: {now}")
            last = now
        time.sleep(1)


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    state = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        guest.agent.put_file(HERE / "display.ps1", SCRIPT)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        clients = tiling.run(platform, Checks())
        platform.add_display()
        platform.wait_for(lambda: len(platform.state().monitors) == 2, 30)
        moved = clients[1:]
        for client in moved:
            platform.activate(client)
            platform.komorebic("move-to-monitor", "1")
            platform.wait_for(
                lambda c=client: platform.state().workspace_of(
                    c.title, monitor=1) is not None)
        watch(platform, moved, "before", 3)
        for width, height in SIZES:
            print(f"PASTE {display(guest, 'set', DEVICE, str(width), str(height))}")
            watch(platform, moved, f"at {width}x{height}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
