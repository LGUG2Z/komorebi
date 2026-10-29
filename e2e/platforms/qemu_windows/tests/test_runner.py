"""The runner of the Windows guest, without a guest."""

import json
import subprocess
from pathlib import Path
from types import SimpleNamespace

import pytest

from platforms.qemu_windows import run, state
from platforms.qemu_windows.guest import Guest
from platforms.qemu_windows.windows_platform import WindowsPlatform


def _git(repo: Path, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(repo), "-c", "user.name=t", "-c", "user.email=t@t", *args],
        capture_output=True, text=True, check=True,
    ).stdout.strip()


@pytest.fixture
def history(tmp_path: Path) -> dict[str, str]:
    """A repository with three commits: before the fix, the fix, after it."""
    _git(tmp_path, "init", "-q")
    commits = {}
    for name in ("before", "fix", "after"):
        (tmp_path / "f").write_text(name)
        _git(tmp_path, "add", "f")
        _git(tmp_path, "commit", "-q", "-m", name)
        commits[name] = _git(tmp_path, "rev-parse", "HEAD")
    commits["repo"] = str(tmp_path)
    return commits


def test_a_commit_with_the_fix_contains_it(
    history: dict[str, str], monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = Path(history["repo"])
    monkeypatch.setenv("KOMOREBI_E2E_COMMIT", history["after"])
    assert run._contains(history["fix"], repo)
    monkeypatch.setenv("KOMOREBI_E2E_COMMIT", history["fix"])
    assert run._contains(history["fix"], repo)


def test_a_commit_before_the_fix_does_not(
    history: dict[str, str], monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("KOMOREBI_E2E_COMMIT", history["before"])
    assert not run._contains(history["fix"], Path(history["repo"]))


def test_an_unknown_commit_keeps_the_marker(
    history: dict[str, str], monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = Path(history["repo"])
    monkeypatch.delenv("KOMOREBI_E2E_COMMIT", raising=False)
    assert not run._contains(history["fix"], repo)
    monkeypatch.setenv("KOMOREBI_E2E_COMMIT", "0" * 40)
    assert not run._contains(history["fix"], repo)


def test_recorded_checks_keep_the_scope(capsys: pytest.CaptureFixture[str]) -> None:
    checks = run.RecordedChecks()
    checks.check("plain", True)
    with checks.scoped("windows"):
        checks.check("scoped", False)
        checks.skip("skipped", "no reason")
        checks.known_failure("known", False, "a bug")

    assert checks.results == [
        {"check": "plain", "passed": True},
        {"check": "scoped", "passed": False, "scope": "windows"},
        {"check": "skipped", "skipped": True, "reason": "no reason", "scope": "windows"},
        {"check": "known", "known_failure": True, "reason": "a bug", "scope": "windows"},
    ]
    assert "[windows] scoped" in capsys.readouterr().out


def test_a_step_wrapper_keeps_the_name_and_passes_keywords() -> None:
    def scenario(platform: str, checks: str, extra: int = 0) -> tuple[str, str, int]:
        return platform, checks, extra

    step = run._with(scenario, extra=3)

    assert step("p", "c") == ("p", "c", 3)
    assert step.__name__ == "scenario"
    assert step.__module__ == scenario.__module__


def test_the_suite_keeps_how_long_each_step_took() -> None:
    from komorebi_vm.core.suite import platform_only, shared

    def scenario() -> str:
        return "opened"

    def extra() -> None:
        return None

    checks = run.RecordedChecks()
    results = run._suite(checks, shared(scenario), platform_only("windows", extra))

    assert results == ["opened", None]
    assert [s["step"] for s in checks.steps] == [
        "[shared] test_runner.scenario",
        "[windows] test_runner.extra",
    ]
    assert all(isinstance(s["seconds"], float) for s in checks.steps)


def test_the_state_key_follows_the_machine_and_not_the_run() -> None:
    first = state._machine()

    assert first == state._machine()
    assert all("/tmp" not in arg for arg in first)
    changed = Guest(Path("golden"), Path("run"), ahci=True, vcpus=8)
    changed.agent_port = 0
    other = changed.qemu_args(Path("ovmf"), Path("run"), Path("run/overlay.qcow2"))
    assert other != first


def _platform(printed: str) -> WindowsPlatform:
    agent = SimpleNamespace(
        exec=lambda *args: {"status": 0, "stdout": printed, "stderr": ""}
    )
    return WindowsPlatform(SimpleNamespace(), agent, Path("."))


def test_exe_ids_have_the_extension() -> None:
    assert _platform("").exe_id("e2e-exe-sent") == "e2e-exe-sent.exe"


def test_a_state_that_is_not_json_is_a_runtime_error_with_what_was_printed() -> None:
    platform = _platform("warning: busy\n{")

    with pytest.raises(RuntimeError, match="did not print JSON.*warning: busy"):
        platform.state()


def test_a_wait_goes_on_past_a_runtime_error_and_raises_the_last_one() -> None:
    platform = _platform("not json")
    calls = []

    def sometimes() -> bool:
        calls.append(1)
        if len(calls) < 3:
            raise RuntimeError("busy")
        return True

    assert platform.wait_for(sometimes, timeout=1)
    with pytest.raises(RuntimeError, match="did not print JSON"):
        platform.wait_for(lambda: platform.state().managed() == 0, timeout=0.2)


def test_the_result_file_names_a_known_json_shape(tmp_path: Path) -> None:
    checks = run.RecordedChecks()
    checks.check("a", True)

    json.dumps(checks.results)


class _Agent:
    """An agent that has one window and records what it is asked to run."""

    def __init__(self, titles: list[str]) -> None:
        self.titles = titles
        self.spawned: list[tuple[str, ...]] = []
        self.executed: list[tuple[str, ...]] = []

    def os_windows(self) -> list[dict]:
        return [{"title": t, "hwnd": 7, "cloaked": False,
                 "frame": {"left": 0, "top": 0, "right": 1, "bottom": 1}}
                for t in self.titles]

    def spawn(self, *args: str) -> None:
        self.spawned.append(args)

    def exec(self, *args: str) -> dict:
        self.executed.append(args)
        return {"status": 0, "stdout": "", "stderr": ""}


def test_a_window_that_renames_itself_is_known_by_the_new_title() -> None:
    agent = _Agent(["opens-as"])
    platform = WindowsPlatform(SimpleNamespace(), agent, Path("."))

    client = platform.spawn("opens-as", rename_to="becomes", after=6)

    assert client.title == "becomes"
    assert agent.spawned[0][-4:] == ("--rename-to", "becomes", "--after", "6")


def test_a_spawn_that_never_shows_raises() -> None:
    platform = WindowsPlatform(SimpleNamespace(), _Agent([]), Path("."), scale=0.01)

    with pytest.raises(RuntimeError, match="no window titled 'ghost'"):
        platform.spawn("ghost")


def test_a_program_name_windows_would_reject_is_rejected() -> None:
    platform = WindowsPlatform(SimpleNamespace(), _Agent(["w"]), Path("."))

    with pytest.raises(ValueError, match="not a program name"):
        platform.spawn("w", exe="a b")
    with pytest.raises(ValueError, match="not a program name"):
        platform.spawn("w", exe="..\\evil")


def test_a_temp_path_is_in_the_guest_and_a_bad_name_is_rejected() -> None:
    agent = _Agent([])
    platform = WindowsPlatform(SimpleNamespace(), agent, Path("."))

    assert platform.temp_path("e2e-resize.json") == "C:\\e2e\\tmp\\e2e-resize.json"
    with pytest.raises(ValueError):
        platform.temp_path("..\\x")


def test_acting_on_a_window_that_is_not_there_raises_a_clear_error() -> None:
    from komorebi_vm.core.platform import Client

    platform = WindowsPlatform(SimpleNamespace(), _Agent([]), Path("."))
    gone = Client("gone")

    for act in (platform.close, platform.minimize, platform.activate,
                lambda c: platform.move_window(c, 1, 1),
                platform.fullscreen, platform.user_maximize):
        with pytest.raises(RuntimeError, match="no window titled 'gone'"):
            act(gone)


def test_a_guest_that_died_says_how_and_a_live_one_says_nothing(tmp_path: Path) -> None:
    guest = Guest(Path("golden"), tmp_path)
    live = subprocess.Popen(["sleep", "5"])
    guest._procs.append(live)
    assert guest.died() is None
    live.kill()
    live.wait()

    dead = subprocess.Popen(["sh", "-c", "kill -ABRT $$"])
    dead.wait()
    dead.args = ["qemu-system-x86_64"]
    (tmp_path / "qemu-system-x86_64.err").write_text("assertion failed\nAborted\n")
    guest._procs = [dead]

    assert "signal 6" in guest.died()
    assert "assertion failed" in guest.died()


def test_a_known_step_turns_its_failures_into_known_failures_and_says_when_it_passes() -> None:
    from komorebi_vm.core.suite import shared

    def user(checks: run.RecordedChecks, holds: bool) -> None:
        checks.check("a", True)
        checks.check("b", holds)

    user.__module__ = "komorebi_vm.core.scenarios.maximizing"
    checks = run.RecordedChecks()

    run._suite(checks, shared(user, checks, False))

    assert checks.failed == 0
    assert len(checks.known) == 1
    assert checks.results[-1]["known_failure"] is True

    run._suite(checks, shared(user, checks, True))

    assert checks.failed == 1
    assert "no longer a known failure" in checks.failures[0]


def test_a_step_that_is_not_known_fails_as_it_did() -> None:
    from komorebi_vm.core.suite import shared

    def other(checks: run.RecordedChecks) -> None:
        checks.check("x", False)

    checks = run.RecordedChecks()
    run._suite(checks, shared(other, checks))

    assert checks.failed == 1
    assert checks.known == []


def test_the_press_points_of_a_resize_are_outside_but_for_the_top() -> None:
    from komorebi_vm.core.geometry import Rect

    platform = _platform("")
    frame = Rect(657, 393, 596, 332)

    assert platform.resize_grab(frame, "left") == (653, 559)
    assert platform.resize_grab(frame, "right") == (1257, 559)
    assert platform.resize_grab(frame, "bottom") == (955, 729)
    # no invisible border on the top: inside the first rows, not the desktop
    assert platform.resize_grab(frame, "top") == (955, 395)
    assert platform.resize_grab(frame, "topleft") == (653, 395)
    assert platform.resize_grab(frame, "topright") == (1257, 395)
    assert platform.resize_grab(frame, "bottomright") == (1257, 729)
