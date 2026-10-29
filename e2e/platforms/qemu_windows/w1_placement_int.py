"""W1: does kfw accept an integer as `initial_window_placement_rules`?

usage: python -m platforms.qemu_windows.w1_placement_int GOLDEN BIN_DIR OUT

Starts komorebi.exe with a config whose rules are the number 2, then the
string "2", and prints whether it stayed up and what it wrote to stderr.
A probe: it records what it saw.
"""

import json
import sys
from pathlib import Path

from .guest import Guest
from .run import BINARIES
from .windows_platform import GUEST_BIN, WindowsPlatform

SCRIPT = (
    "$p = Start-Process -FilePath '{exe}' -ArgumentList "
    "'--clean-state','--config','C:\\e2e\\komorebi.json' "
    "-RedirectStandardError C:\\e2e\\err.txt -PassThru -WindowStyle Hidden; "
    "Start-Sleep 6; "
    "\"exited=$($p.HasExited) code=$($p.ExitCode)\"; "
    "Get-Content C:\\e2e\\err.txt; "
    "if (-not $p.HasExited) {{ & '{c}' stop | Out-Null; Start-Sleep 2 }}"
)


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    out.mkdir(parents=True, exist_ok=True)
    with Guest(golden, out / "guest") as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out)
        for rules in (2, "2"):
            config = {"monitors": [{"workspaces": [
                {"name": "a", "layout": "BSP",
                 "initial_window_placement_rules": rules}]}]}
            local = out / "komorebi.json"
            local.write_text(json.dumps(config))
            guest.agent.put_file(local, "C:\\e2e\\komorebi.json")
            result = guest.agent.exec(
                "powershell", "-NoProfile", "-Command",
                SCRIPT.format(
                    exe=f"{GUEST_BIN}\\komorebi.exe",
                    c=f"{GUEST_BIN}\\komorebic.exe",
                ),
            )
            print(f"PASTE rules={rules!r}:")
            for line in (result["stdout"] + result["stderr"]).splitlines():
                print("PASTE   ", line)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
