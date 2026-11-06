"""Runs neutral scenarios against a fresh macOS guest.

usage: python -m platforms.tart_macos.run GOLDEN BIN_DIR OUT SCENARIO...

GOLDEN is the name of the golden Tart image, BIN_DIR holds komorebi, komorebic and
komorebi-testwin (built for macOS, see `build-bins.sh`), OUT gets the run's artifacts. Every
scenario gets its own guest, a clone of the golden image. Those that take clients start from the
windows that `tiling` opens.

Every run writes OUT/<scenario>/result.json, naming the scenario, the port, the commit under test
and each check, so that a session can be matched with its run.

With `KOMOREBI_E2E_SCALE=1.5` every wait of a scenario is 1.5 times as long, for a busy host.
With `KOMOREBI_E2E_LOG_LEVEL=debug` the daemon logs at that level.
"""

import importlib
import json
import os
import sys
import time
import traceback
from pathlib import Path

from komorebi_vm.core.suite import shared
from komorebi_vm.runner import RecordedChecks, commit, run_steps

from .guest import Guest
from .macos_platform import MacosPlatform

BINARIES = ["komorebi", "komorebic", "komorebi-testwin"]

# scenarios that take the windows `tiling` opens
WITH_CLIENTS = {
    "focus", "moving", "stacks", "workspaces", "resize", "monocle", "closing",
    "commands", "lifecycle", "layouts", "floating", "minimizing", "padding",
    "maximizing", "workspace_moves", "resize_commands", "promoting",
    "flipping", "pausing", "tiling_off", "lifecycle_hidden",
    "scrolling", "handover", "fullscreen",
}

# what a scenario file has besides `run`, which its suite runs after it
MORE = {
    "resize": ("edge_cases",),
    "lifecycle_hidden": ("crash",),
    "monocle": ("focus_behaviour",),
}

# scenarios a macOS guest cannot run, and why: they are skipped, not failed
UNSUPPORTED = {
    "hotplug": "a macOS guest has one display",
    "unknown_monitor": "a macOS guest has one display",
    "display_change": "a macOS guest has one display",
    "monitors": "a macOS guest has one display",
    "focus_follows_mouse": "a macOS guest has no pointer, and the daemon has no focus follows mouse",
    "locking": "a macOS guest has no lock screen to test",
    "wallpaper": "the wallpaper of a macOS guest is not read yet",
    "rules_class": "a macOS window has no class",
    "placement": "the macOS daemon has no initial_window_placement_rules",
}

# the checks of a scenario that are about a difference of the macOS daemon, by a part of their
# description, and the difference; the others of the scenario are checks as any. A mark whose check
# passes is a failure, to say that it can go.
KNOWN_CHECKS = {
    # stacks
    "exactly one window is put away": "STACK",
    "after 'cycle-stack next' the other window of the stack is in view and the first is away": "STACK",
    "cycling back, the first is in view again": "STACK",
    # fullscreen
    "it does not take the window out of fullscreen": "FULLSCREEN",
    "not on any other window": "FULLSCREEN",
    # layout_flip
    "BSP, 3 windows, layout_flip Vertical: ... each where the mirror of its place is": "MARGINS",
    "BSP, 3 windows, layout_flip HorizontalAndVertical: ... each where the mirror of its place is": "MARGINS",
    # the class of a window
    "an ignore rule by class matches is not managed": "CLASS",
    "keeps its place (class)": "CLASS",
    "the state has the class a window was given": "CLASS",
}
DIFFERENCES = {
    "STACK": (
        "the macOS daemon stacks by raising the focused window over the others: the other windows "
        "of a stack stay on the display, at the same place, where Windows and KDE put them away"
    ),
    "FULLSCREEN": (
        "the macOS daemon has no fullscreen detection: it tiles a window that covers the whole "
        "display with the others"
    ),
    "MARGINS": (
        "the daemon tiles 10 px below the top of the work area the system reports and 12 px above "
        "its bottom, so a vertical mirror is a few pixels off"
    ),
    "CLASS": (
        "a macOS window has no class: the daemon matches the role of the window, which every "
        "window of the test program shares"
    ),
}
KNOWN_CHECKS = {key: DIFFERENCES[name] for key, name in KNOWN_CHECKS.items()}


class GuestDied(RuntimeError):
    """The guest exited while a scenario was running: not what the daemon did."""


def run_one(golden: str, bin_dir: Path, out: Path, name: str) -> RecordedChecks:
    out.mkdir(parents=True, exist_ok=True)
    scenario = importlib.import_module(f"komorebi_vm.core.scenarios.{name}")
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")

    checks = RecordedChecks(KNOWN_CHECKS)
    print(f"== {name}")
    began = time.monotonic()
    timings: dict[str, float] = {}

    def lap(phase: str) -> None:
        now = time.monotonic()
        timings[phase] = round(now - began - sum(timings.values()), 1)

    with Guest(golden, out / "guest") as guest:
        lap("guest")
        guest.upload(bin_dir, BINARIES)
        lap("upload")
        platform = MacosPlatform(
            guest.agent, out, scale=float(os.environ.get("KOMOREBI_E2E_SCALE", "1"))
        )
        try:
            platform.start_komorebi(log_level=os.environ.get("KOMOREBI_E2E_LOG_LEVEL"))
        except Exception:
            _diagnose(platform, out)
            raise
        lap("daemon")
        try:
            if name in WITH_CLIENTS:
                clients = tiling.run(platform, checks)
                run_steps(
                    checks,
                    shared(scenario.run, platform, checks, clients),
                    *(
                        shared(getattr(scenario, more), platform, checks, clients)
                        for more in MORE.get(name, ())
                    ),
                )
            else:
                run_steps(
                    checks,
                    shared(scenario.run, platform, checks),
                    *(
                        shared(getattr(scenario, more), platform, checks)
                        for more in MORE.get(name, ())
                    ),
                )
            checks.stale_marks()
        except Exception as e:
            died = guest.died()
            if died:
                # the guest is gone, so there is nothing to fetch from it
                raise GuestDied(died) from e
            _diagnose(platform, out)
            raise
        finally:
            lap("scenario")
            print(f"     seconds: {timings}")
            try:
                platform.screenshot(f"final-{name}")
            except Exception as e:  # noqa: BLE001 - a screenshot is for the reader, not a result
                print(f"     no screenshot: {e}")
            (out / "result.json").write_text(json.dumps({
                "scenario": name,
                "port": "macos",
                "commit": commit(),
                "passed": checks.passed,
                "failed": checks.failed,
                "skipped": len(checks.skipped),
                "known_failures": len(checks.known),
                "start_retries": platform.start_notes,
                "seconds": timings,
                "steps": checks.steps,
                "checks": checks.results,
            }, indent=2))
    return checks


def _diagnose(platform: MacosPlatform, out: Path) -> None:
    """After a failure: the daemon's output and log, the processes, and a screenshot, into `out`."""
    try:
        (out / "ps.txt").write_text(platform.agent_exec("ps", "-axo", "pid,etime,command")["stdout"])
        platform.screenshot("failure")
    except Exception as e:  # noqa: BLE001 - diagnosis must not hide the failure it is for
        print(f"     no diagnosis: {e}")
    for name, remote in (("komorebi.out", "/Users/admin/e2e/komorebi.out"),):
        try:
            (out / name).write_bytes(platform.agent_get_file(remote))
        except Exception as e:  # noqa: BLE001
            print(f"     no {name}: {e}")


def main(argv: list[str]) -> int:
    golden, bin_dir, out, *names = argv
    failed = 0
    for name in names:
        if name in UNSUPPORTED:
            print(f"== {name}: skipped, {UNSUPPORTED[name]}")
            continue
        try:
            try:
                checks = run_one(golden, Path(bin_dir), Path(out) / name, name)
            except GuestDied as e:
                # the guest itself went: not a result about the daemon, so the scenario runs again
                print(f"{name}: THE GUEST DIED ({e}); running it again")
                checks = run_one(golden, Path(bin_dir), Path(out) / name, name)
        except Exception:  # noqa: BLE001 - a scenario that cannot run is a failure of its own
            traceback.print_exc()
            print(f"{name}: did not run, counted as 1 failed")
            failed += 1
            continue
        print(f"{name}: passed {checks.passed} failed {checks.failed}")
        if checks.skipped:
            print(f"{name}: skipped {len(checks.skipped)}")
        if checks.known:
            print(f"{name}: known failures {len(checks.known)}")
        failed += checks.failed
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
