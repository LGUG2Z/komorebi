"""W1: can the second QXL display be hotplugged and unplugged at runtime?

usage: python -m platforms.qemu_windows.w1_hotplug GOLDEN QXL_ISO OUT [BIN_DIR]

The guest boots with one display, a hotplug-capable PCIe root port and the
driver CD; `device_add` then adds a QXL device through QMP and `device_del`
removes it. Prints what the agent and (with BIN_DIR) kfw saw at each step.
A probe: it records what it saw.
"""

import sys
import time
from pathlib import Path

from .agent import AgentError
from .guest import Guest
from .windows_platform import WindowsPlatform

POWERSHELL = ["powershell", "-NoProfile", "-Command"]


def seen(guest: Guest, platform: WindowsPlatform | None, label: str) -> None:
    displays = guest.agent.displays()
    line = f"PASTE {label}: windows displays {len(displays)}"
    if platform is not None:
        line += f", kfw monitors {len(platform.state().monitors)}"
    print(line)
    for d in displays:
        print("PASTE    ", d["monitor"], "primary" if d["primary"] else "")


def wait_displays(guest: Guest, count: int, timeout: float = 40) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            if len(guest.agent.displays()) == count:
                return True
        except (OSError, ValueError, AgentError) as e:
            # the agent dropped the connection: say so, and reconnect
            print("PASTE   agent call failed:", type(e).__name__, e)
            try:
                guest.agent.connect(timeout=10)
            except AgentError:
                print("PASTE   agent did not come back in 10 s")
        time.sleep(1)
    return False


def main(argv: list[str]) -> int:
    golden, qxl_iso, out = (Path(a) for a in argv[:3])
    bin_dir = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    extra = [
        "-device", "pcie-root-port,id=rp1,chassis=1,slot=1",
        "-drive", f"if=none,id=qd,file={qxl_iso},media=cdrom,readonly=on",
        "-device", "ide-cd,drive=qd,bus=ide.0",
    ]
    with Guest(golden, out / "guest", extra=extra) as guest:
        platform = None
        if bin_dir:
            guest.upload(bin_dir, ["komorebi.exe", "komorebic.exe"])
            platform = WindowsPlatform(guest.qmp, guest.agent, out)
            platform.start_komorebi()
        time.sleep(3)
        seen(guest, platform, "before")

        found = guest.agent.exec(
            *POWERSHELL,
            "(Get-PSDrive -PSProvider FileSystem | Where-Object "
            "{ Test-Path (Join-Path $_.Root 'qxldod.inf') }).Root",
        )["stdout"].strip()
        # the driver goes into the store first, so the device gets it on
        # arrival, as it will from the golden image
        guest.agent.exec("pnputil", "/add-driver", f"{found}qxldod.inf")

        t0 = time.monotonic()
        guest.qmp.execute(
            "device_add", driver="qxl", id="qxl2", bus="rp1",
            ram_size_mb=64, vram_size_mb=64, vgamem_mb=16,
        )
        ok = wait_displays(guest, 2)
        took = time.monotonic() - t0
        print(f"PASTE device_add: second display after {took:.1f}s:", ok)
        time.sleep(3)
        guest.qmp.screendump(out / "after-add.png")
        seen(guest, platform, "after device_add")

        t0 = time.monotonic()
        guest.qmp.execute("device_del", id="qxl2")
        ok = wait_displays(guest, 1)
        took = time.monotonic() - t0
        print(f"PASTE device_del: one display again after {took:.1f}s:", ok)
        time.sleep(3)
        seen(guest, platform, "after device_del")
        if platform is not None:
            t0 = time.monotonic()
            while time.monotonic() - t0 < 45:
                if len(platform.state().monitors) == 1:
                    break
                time.sleep(1)
            n = len(platform.state().monitors)
            print(f"PASTE kfw monitors {n} after a further "
                  f"{time.monotonic() - t0:.0f}s")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
