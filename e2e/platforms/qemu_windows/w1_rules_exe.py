"""W1: does an exe rule on kfw need the extension in its id?

usage: python -m platforms.qemu_windows.w1_rules_exe GOLDEN BIN_DIR OUT [STATE]

Runs the `rules_exe` scenario with the rule ids spelled `NAME.exe`, which is
how kfw names the exe of a window, and prints its checks. A probe.
"""

import importlib
import inspect
import sys
from pathlib import Path

from .guest import Guest
from .run import BINARIES, RecordedChecks
from .windows_platform import WindowsPlatform


def main(argv: list[str]) -> int:
    golden, bin_dir, out = (Path(a) for a in argv[:3])
    state = Path(argv[3]) if len(argv) > 3 else None
    out.mkdir(parents=True, exist_ok=True)
    module = importlib.import_module("komorebi_vm.core.scenarios.rules_exe")
    source = inspect.getsource(module)
    for name in ("sent", "left"):
        source = source.replace(
            f'"exe", "e2e-exe-{name}"', f'"exe", "e2e-exe-{name}.exe"'
        )
    scope: dict = {}
    exec(compile(source, "rules_exe_exe", "exec"), scope)
    with Guest(golden, out / "guest", state=state) as guest:
        guest.upload(bin_dir, BINARIES)
        platform = WindowsPlatform(guest.qmp, guest.agent, out, scale=1.5)
        platform.start_komorebi()
        checks = RecordedChecks()
        scope["run"](platform, checks)
        print(f"PASTE passed {checks.passed} failed {checks.failed}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
