"""W1: which display devices and modes does the guest have, and can one change?

usage: python -m platforms.qemu_windows.w1_display_modes GOLDEN OUT STATE

Plugs in a second display, prints the devices of the guest with the current
mode of each and the modes it lists, then tries to change the mode of each
device to the first listed other than the current one, and prints the
size the operating system reports for it afterwards. A probe: it prints what
it saw.
"""

import re
import sys
import time
from pathlib import Path

from .guest import Guest
from .windows_platform import WindowsPlatform

HERE = Path(__file__).parent
SCRIPT = "C:\\e2e\\display.ps1"


def display(guest: Guest, *args: str) -> str:
    result = guest.agent.exec(
        "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
        "-File", SCRIPT, *args,
    )
    return (result["stdout"] + result["stderr"]).strip()


def main(argv: list[str]) -> int:
    golden, out = Path(argv[0]), Path(argv[1])
    state = Path(argv[2]) if len(argv) > 2 else None
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest", state=state) as guest:
        guest.agent.exec("cmd", "/c", "mkdir C:\\e2e 2>nul")
        guest.agent.put_file(HERE / SCRIPT.split("\\")[-1], SCRIPT)
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        platform.add_display()
        time.sleep(5)
        listing = display(guest, "list")
        for line in listing.splitlines():
            print(f"PASTE {line}")
        device = None
        for line in listing.splitlines():
            if line.startswith("device "):
                device = line.split()[1]
            elif line.strip().startswith("current"):
                current = re.search(r"(\d+x\d+)", line).group(1)
            elif line.strip().startswith("modes:") and device:
                other = [m for m in line.split()[1:] if m != current]
                if other:
                    wanted = sorted(other, key=lambda m: int(m.split("x")[0]))[0]
                    w, h = wanted.split("x")
                    print(f"PASTE {display(guest, 'set', device, w, h)}")
                    time.sleep(3)
                    print("PASTE displays now: " + ", ".join(
                        f"{d['monitor']}" for d in guest.agent.displays()))
                    print("PASTE " + "; ".join(
                        l for l in display(guest, "list").splitlines()
                        if "current" in l))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
