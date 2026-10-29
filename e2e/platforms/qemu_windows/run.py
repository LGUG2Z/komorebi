"""Runs neutral scenarios against a fresh Windows guest.

usage: python -m platforms.qemu_windows.run GOLDEN BIN_DIR OUT SCENARIO...

GOLDEN is the golden image directory, BIN_DIR holds komorebi.exe,
komorebic.exe and komorebi-testwin.exe, OUT gets the run's artifacts.
Every scenario gets its own guest. Those that take clients start from the
windows that `tiling` opens.

With `KOMOREBI_E2E_RECORDER=DIR` (the folder of komorebi-recorder.exe) the
daemon runs at trace level and komorebi-recorder records the scenario in the
guest, with window tracking; the session comes back as OUT/recording.zip. Every
run also writes OUT/result.json, naming the scenario, the port, the kfw commit
and each check, so that a session can be matched with its run.

With `KOMOREBI_E2E_SCALE=1.5` every wait of a scenario is 1.5 times as long, for a
busy host (the daemon is slow to answer, and kfw is slow to drop a monitor).

With `KOMOREBI_E2E_STATE=DIR` every guest is restored from a state saved there
(made on the first run, and again when the golden image, QEMU or guest.py
changed) instead of booting: about 3 s instead of 50 to 90.

With `KOMOREBI_E2E_CAPTURE=DIR` the raw `komorebic state` is saved to DIR
after `tiling` (state-kfw-tiled.json) and after one window is stacked onto
another (state-kfw-stack.json) and after the focused one is floated
(state-kfw-float.json), to be used as fixtures.
"""

import importlib
import json
import os
import subprocess
import sys
import time
import traceback
import zipfile
from pathlib import Path

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.suite import platform_only, run_suite, shared

from .agent import AgentError, AgentLost
from .guest import Guest
from .state import ensure
from .windows_platform import WindowsPlatform

# `move_container_to_monitor` takes the focused container off its monitor
# before it looks the target monitor up, so a monitor that is not there loses
# the window; the window stays on screen, unmanaged
UNKNOWN_MONITOR_BUG = (
    "kfw drops the focused window from management when the target monitor "
    "does not exist"
)


# `PlacementTarget` deserializes with `String::deserialize`, so the number 2
# is rejected ("data did not match any variant of untagged enum
# InitialWindowPlacementRules") although the type's docs allow an integer;
# the string "2" works
PLACEMENT_INTEGER_CASE = "by the number of the place, counted from 1"
PLACEMENT_INTEGER_BUG = (
    "kfw rejects a number as initial_window_placement_rules and exits at "
    "start, a string is accepted"
)


# steps whose failing checks are known differences of kfw, and why: a failing
# check of such a step is recorded as a known failure, and a step that has no
# failing check says the marker can go
KNOWN_STEPS = {
    "maximizing.user": (
        "kfw sets maximized_window only from toggle-maximize; a user's "
        "maximize (SC_MAXIMIZE) is not taken as the maximized window"
    ),
    "hotplug.user_move": (
        "kfw relocates a window to another monitor only when a drag ends "
        "(MoveResizeEnd); a move by any other means is ignored"
    ),
}


# the commit of komorebi's `next` that fixed what a step is known to fail, and
# what it still fails on a build that has it: the marker is the first reason
# before it and the second after it
KNOWN_STEPS_FIXED_BY = {
    "maximizing.user": (
        "3fa3d429",
        "kfw tracks a user's maximize, but toggle-float (it floats the window "
        "and ends the maximize) and stack left/right (they stack the others) "
        "are not ignored while a window is maximized",
    ),
}


class GuestDied(RuntimeError):
    """QEMU exited while a scenario was running: not what kfw did."""


class RecordedChecks(Checks):
    """`Checks` that also keeps each check, for the result file."""

    def __init__(self) -> None:
        super().__init__()
        self.results: list[dict[str, object]] = []
        # how long each step of the suite took, for the result file
        self.steps: list[dict[str, object]] = []
        # the reason the failing checks of the step being run are known ones
        self.expected: str | None = None

    def _record(self, **fields: object) -> None:
        if self.scope:
            fields["scope"] = self.scope
        self.results.append(fields)

    def ok(self, description: str) -> None:
        super().ok(description)
        self._record(check=description, passed=True)

    def bad(self, description: str) -> None:
        if self.expected:
            self.known.append(f"{self.labelled(description)} ({self.expected})")
            print(f"  known failure {self.labelled(description)}: {self.expected}")
            self._record(
                check=description, known_failure=True, reason=self.expected
            )
            return
        super().bad(description)
        self._record(check=description, passed=False)

    def skip(self, description: str, reason: str) -> None:
        super().skip(description, reason)
        self._record(check=description, skipped=True, reason=reason)

    def known_failure(
        self, description: str, condition: bool, reason: str
    ) -> bool:
        holds = super().known_failure(description, condition, reason)
        if not holds:
            self._record(
                check=description, known_failure=True, reason=reason
            )
        return holds


def _suite(checks: "RecordedChecks", *steps):
    """`run_suite`, one step at a time, keeping how long each took."""
    results: list[object] = []
    for step in steps:
        began = time.monotonic()
        counted = checks.passed + checks.failed + len(checks.known)
        fixed = KNOWN_STEPS_FIXED_BY.get(step.name)
        checks.expected = (
            fixed[1] if fixed and _contains(fixed[0])
            else KNOWN_STEPS.get(step.name)
        )
        known_before = len(checks.known)
        try:
            results.extend(run_suite(checks, step))
        finally:
            reason, checks.expected = checks.expected, None
        ran = checks.passed + checks.failed + len(checks.known) > counted
        if reason and ran and len(checks.known) == known_before:
            checks.bad(
                f"{step.name}: nothing failed, so it is no longer a known "
                "failure"
            )
        checks.steps.append({
            "step": f"[{step.scope}] {step.name}",
            "seconds": round(time.monotonic() - began, 1),
        })
    return results


def _with(fn, **kwargs):
    """`fn` with keyword arguments, for a suite step, which takes positional
    ones only; it keeps the name the step prints."""
    def step(*args):
        return fn(*args, **kwargs)

    step.__module__ = fn.__module__
    step.__name__ = fn.__name__
    return step


def _contains(fix: str, repo: Path | None = None) -> bool:
    """Whether the commit under test has the commit `fix` in its history, so that
    a marker for the bug it fixed no longer applies. Not known: no."""
    commit = os.environ.get("KOMOREBI_E2E_COMMIT")
    if not commit:
        return False
    here = repo or Path(__file__).resolve().parent
    git = subprocess.run(
        ["git", "-C", str(here), "merge-base", "--is-ancestor", fix, commit],
        capture_output=True,
    )
    return git.returncode == 0


# the commit that fixed each bug a marker is for (komorebi's `next`)
UNKNOWN_MONITOR_FIX = "837ca257"
PLACEMENT_INTEGER_FIX = "3b424dd7"


def _commit() -> str:
    """The kfw commit under test: set `KOMOREBI_E2E_COMMIT` to the commit the
    binaries were built from; otherwise this tree's HEAD, which may not be it."""
    if os.environ.get("KOMOREBI_E2E_COMMIT"):
        return os.environ["KOMOREBI_E2E_COMMIT"]
    here = Path(__file__).resolve().parent
    try:
        git = subprocess.run(
            ["git", "-C", str(here), "rev-parse", "HEAD"],
            capture_output=True, text=True,
        )
    except FileNotFoundError:
        return "unknown"
    return git.stdout.strip() or "unknown"


BINARIES = ["komorebi.exe", "komorebic.exe", "komorebi-testwin.exe"]
WITH_CLIENTS = {
    "focus", "moving", "stacks", "workspaces", "resize", "monocle", "closing",
    "commands", "hotplug", "unknown_monitor", "lifecycle",
    "focus_follows_mouse", "layouts", "floating", "minimizing", "padding",
    "maximizing", "workspace_moves", "resize_commands", "promoting",
    "flipping", "pausing", "tiling_off", "lifecycle_hidden",
    "scrolling", "locking", "handover", "fullscreen", "display_change",
}

# what a scenario file has besides `run`, which its suite runs after it
MORE = {
    "hotplug": ("user_move",),
    "resize": ("edge_cases",),
    "maximizing": ("user",),
    "lifecycle_hidden": ("crash",),
    "monocle": ("focus_behaviour",),
    "monitors": ("at_cursor",),
    "scrolling": ("columns", "two_monitors"),
}

# what only Windows can check, `extras/<name>.py`, after the tiling windows
EXTRAS = {
    "windows_padding": "padding", "windows_cloaking": "cloaking",
    "windows_config_file": "config_file", "windows_tracking": "tracking",
    "windows_second_display": "second_display",
    "windows_display_change": "display_change",
}

# a small configuration for the sweep's `replace-configuration`
SWEEP_CONFIG = '{"monitors":[{"workspaces":[{"name":"again","layout":"BSP"}]}]}'
SWEEP_CONFIG_PATH = "C:\\e2e\\sweep.json"


def _capture(platform: WindowsPlatform, directory: Path) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "state-kfw-tiled.json").write_text(
        platform.komorebic("state")
    )
    # the last window opened has the focus: stack it onto its neighbor
    platform.komorebic("stack", "left")
    platform.wait_for(lambda: len(platform.state().containers()) == 2)
    time.sleep(1)
    (directory / "state-kfw-stack.json").write_text(platform.komorebic("state"))
    # then float the focused window, which leaves the stack
    platform.komorebic("toggle-float")
    time.sleep(1)
    (directory / "state-kfw-float.json").write_text(platform.komorebic("state"))


RECORD_SECONDS = 100


def _start_recorder(
    platform: WindowsPlatform, recorder_dir: Path, guest: Guest, out: Path
) -> None:
    """Starts komorebi-recorder in the guest for a fixed time."""
    guest.upload(recorder_dir, ["komorebi-recorder.exe"])
    doctor = platform.agent_exec("C:\\e2e\\bin\\komorebi-recorder.exe", "doctor")
    (out / "recorder-doctor.txt").write_text(
        f"status {doctor['status']}\n{doctor['stdout']}{doctor['stderr']}"
    )
    args = ["record", "--duration", str(RECORD_SECONDS), "--window-track",
            "--out", "C:\\e2e\\rec"]
    if os.environ.get("RECORD_NO_VIDEO"):
        args.append("--no-video")
    platform.agent_spawn("C:\\e2e\\bin\\komorebi-recorder.exe", *args)


def _collect_recording(platform: WindowsPlatform, out: Path) -> None:
    """Waits for the recorder to finish, then brings the session home."""
    def gone() -> bool:
        listing = platform.agent_exec("tasklist")["stdout"]
        return "komorebi-recorder.exe" not in listing

    platform.wait_for(gone, timeout=RECORD_SECONDS + 60)
    platform.agent_exec(
        "powershell", "-NoProfile", "-Command",
        "Compress-Archive -Force C:\\e2e\\rec C:\\e2e\\rec.zip",
    )
    archive = out / "recording.zip"
    archive.write_bytes(platform.agent_get_file("C:\\e2e\\rec.zip"))
    # Windows PowerShell 5 writes entry names with backslashes
    with zipfile.ZipFile(archive) as z:
        for entry in z.infolist():
            target = out / "session" / entry.filename.replace("\\", "/")
            if entry.is_dir() or entry.filename.endswith("\\"):
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(z.read(entry))


def _fetch_logs(guest: Guest, out: Path) -> None:
    """After a failure: bring home the agent's log and the daemon's."""
    try:
        guest.agent.connect(timeout=20)
        guest.agent.exec(
            "powershell", "-NoProfile", "-Command",
            "Copy-Item $env:TEMP\\komorebi_plaintext.log.* C:\\e2e\\"
            "komorebi.log -ErrorAction SilentlyContinue",
        )
        for name, remote in (("agent.log", "C:\\e2e\\agent.log"),
                             ("komorebi.log", "C:\\e2e\\komorebi.log")):
            try:
                (out / name).write_bytes(guest.agent.get_file(remote))
            except AgentError as e:
                print(f"     no {name}: {e}")
    except (AgentError, OSError, ValueError) as e:
        print(f"     no logs: {e}")


def _diagnose_start(
    platform: WindowsPlatform, guest: Guest, out: Path
) -> None:
    """After a daemon that did not answer: the processes, the daemon's log,
    the agent's log and a screenshot, into `out`."""
    try:
        processes = guest.agent.exec(
            "cmd", "/c", "tasklist /v /fo csv"
        )["stdout"]
        (out / "tasklist.csv").write_text(processes)
        platform.screenshot("start-failed")
    except Exception as e:
        print(f"     no diagnosis of the start: {e}")
    _fetch_logs(guest, out)


def run_one(
    golden: Path, bin_dir: Path, out: Path, name: str,
    state: Path | None = None,
) -> RecordedChecks:
    out.mkdir(parents=True, exist_ok=True)
    module = "commands" if name == "unknown_monitor" else name
    scenario = None
    if name not in EXTRAS:
        scenario = importlib.import_module(
            f"komorebi_vm.core.scenarios.{module}"
        )
    tiling = importlib.import_module("komorebi_vm.core.scenarios.tiling")
    capture = os.environ.get("KOMOREBI_E2E_CAPTURE")

    checks = RecordedChecks()
    print(f"== {name}")
    began = time.monotonic()
    timings: dict[str, float] = {}

    def lap(phase: str) -> None:
        now = time.monotonic()
        timings[phase] = round(now - began - sum(timings.values()), 1)

    with Guest(golden, out / "guest", state=state) as guest:
        lap("guest")
        guest.upload(bin_dir, BINARIES)
        lap("upload")
        platform = WindowsPlatform(
            guest.qmp, guest.agent, out,
            scale=float(os.environ.get("KOMOREBI_E2E_SCALE", "1")),
        )
        recorder = os.environ.get("KOMOREBI_E2E_RECORDER")
        try:
            platform.start_komorebi(
                log_level="trace" if recorder
                else os.environ.get("KOMOREBI_E2E_LOG_LEVEL")
            )
        except Exception:
            # what the guest was doing when the daemon did not answer
            _diagnose_start(platform, guest, out)
            raise
        lap("daemon")
        if recorder:
            _start_recorder(platform, Path(recorder), guest, out)
        try:
            if name in EXTRAS:
                extra = importlib.import_module(
                    f"platforms.qemu_windows.extras.{EXTRAS[name]}"
                )
                clients = _suite(
                    checks, shared(tiling.run, platform, checks)
                )[0]
                _suite(checks, platform_only(
                    "windows", extra.run, platform, checks, clients
                ))
                return checks
            if name in WITH_CLIENTS:
                clients = tiling.run(platform, checks)
                if capture and name == "stacks":
                    _capture(platform, Path(capture))
                    return checks
                kwargs = {}
                if name == "resize" and os.environ.get("RESIZE_DX"):
                    kwargs["dx"] = int(os.environ["RESIZE_DX"])
                if name == "commands":
                    config = out / "sweep.json"
                    config.write_text(SWEEP_CONFIG)
                    guest.agent.put_file(config, SWEEP_CONFIG_PATH)
                    if os.environ.get("SWEEP_TWO_DISPLAYS"):
                        platform.add_display()
                        time.sleep(5)
                    _suite(
                        checks,
                        shared(scenario.layouts, platform, checks, clients),
                        shared(
                            scenario.sweep, platform, checks, clients,
                            SWEEP_CONFIG_PATH,
                        ),
                        shared(scenario.prints, platform, checks),
                    )
                    return checks
                if name == "unknown_monitor":
                    commands = importlib.import_module(
                        "komorebi_vm.core.scenarios.commands"
                    )
                    _suite(checks, shared(
                        _with(
                            commands.unknown_monitor,
                            known_failure=None
                            if _contains(UNKNOWN_MONITOR_FIX)
                            else UNKNOWN_MONITOR_BUG,
                        ),
                        platform, checks, clients,
                    ))
                    return checks
                if name == "closing":
                    # the window that was opened last
                    kwargs["closing"] = clients[-1]
                if name == "focus_follows_mouse":
                    # two tiles that are in view and do not overlap
                    _suite(checks, shared(
                        scenario.run, platform, checks, *clients[:2]
                    ))
                    return checks
                _suite(
                    checks,
                    shared(
                        _with(scenario.run, **kwargs), platform, checks,
                        clients,
                    ),
                    *(
                        shared(getattr(scenario, more), platform, checks, clients)
                        for more in MORE.get(name, ())
                    ),
                )
            elif name == "placement":
                _suite(checks, shared(
                    _with(scenario.run, known_failures=None
                          if _contains(PLACEMENT_INTEGER_FIX) else {
                              PLACEMENT_INTEGER_CASE: PLACEMENT_INTEGER_BUG
                          }),
                    platform, checks,
                ))
            else:
                _suite(
                    checks,
                    shared(scenario.run, platform, checks),
                    *(
                        shared(getattr(scenario, more), platform, checks)
                        for more in MORE.get(name, ())
                    ),
                )
        except AgentLost as e:
            print(f"     AGENT LOST: {e}")
            checks.bad(str(e))
            _fetch_logs(guest, out)
        except Exception as e:
            died = guest.died()
            if died:
                # the guest is gone, so there is nothing to fetch from it
                raise GuestDied(died) from e
            _fetch_logs(guest, out)
            raise
        finally:
            lap("scenario")
            print(f"     seconds: {timings}")
            try:
                platform.screenshot(f"final-{name}")
            except Exception:
                pass
            if recorder:
                _collect_recording(platform, out)
            # matches a recorder session directory with the run that made it
            (out / "result.json").write_text(json.dumps({
                "scenario": name,
                "port": "kfw",
                "commit": _commit(),
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


def main(argv: list[str]) -> int:
    golden, bin_dir, out, *names = argv
    failed = 0
    # the commit the binaries were built from, which build-ref.sh writes
    commit_file = Path(bin_dir) / "commit"
    if commit_file.exists() and not os.environ.get("KOMOREBI_E2E_COMMIT"):
        os.environ["KOMOREBI_E2E_COMMIT"] = commit_file.read_text().strip()
    state = None
    if os.environ.get("KOMOREBI_E2E_STATE"):
        state = ensure(Path(golden), Path(os.environ["KOMOREBI_E2E_STATE"]))
    for name in names:
        try:
            try:
                checks = run_one(
                    Path(golden), Path(bin_dir), Path(out) / name, name, state
                )
            except GuestDied as e:
                # QEMU itself aborted (it has, while a display was plugged in):
                # not a result about kfw, so the scenario runs once more
                print(f"{name}: THE GUEST DIED ({e}); running it again")
                checks = run_one(
                    Path(golden), Path(bin_dir), Path(out) / name, name, state
                )
        except Exception:
            # a scenario that cannot run is a failure of its own, and the
            # others still run
            traceback.print_exc()
            print(f"{name}: did not run, counted as 1 failed")
            failed += 1
            continue
        print(f"{name}: passed {checks.passed} failed {checks.failed}")
        if checks.known:
            print(f"{name}: known failures {len(checks.known)}")
        failed += checks.failed
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
