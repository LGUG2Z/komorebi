"""The macOS port, without a guest: what it decides before and after it asks the agent."""

import importlib
from pathlib import Path
from typing import Any

import pytest
from komorebi_vm.core.platform import Client

from platforms.tart_macos import run
from platforms.tart_macos.macos_platform import (
    IGNORED_APPS,
    MacosPlatform,
    with_ignored_apps,
)


class FakeAgent:
    """An agent with a desktop of one 1280 by 800 display and the windows it is given."""

    def __init__(self, windows: list[dict[str, Any]] | None = None) -> None:
        self.windows = windows or []
        self.calls: list[tuple[str, tuple, dict]] = []

    def os_windows(self) -> list[dict[str, Any]]:
        return self.windows

    def displays(self) -> list[dict[str, Any]]:
        frame = {"left": 0, "top": 0, "right": 1280, "bottom": 800}
        return [{"monitor": frame, "work_area": {**frame, "top": 30}, "primary": True}]

    def exec(self, *args: str) -> dict[str, Any]:
        self.calls.append(("exec", args, {}))
        return {"status": 0, "stdout": "", "stderr": ""}

    def call(self, verb: str, **fields: Any) -> Any:
        self.calls.append((verb, (), fields))
        return True


def window(title: str, left: int, top: int, w: int, h: int, minimized: bool = False) -> dict:
    return {
        "title": title, "minimized": minimized, "focused": False,
        "frame": {"left": left, "top": top, "right": w, "bottom": h},
    }


def platform(agent: FakeAgent, tmp_path: Path) -> MacosPlatform:
    return MacosPlatform(agent, tmp_path)  # type: ignore[arg-type]


def test_the_guests_own_programs_are_left_to_themselves() -> None:
    merged = with_ignored_apps({"monitors": []})
    ids = [rule["id"] for rule in merged["ignore_rules"]]
    assert ids == list(IGNORED_APPS)
    assert all(rule["kind"] == "Exe" for rule in merged["ignore_rules"])


def test_a_scenarios_own_ignore_rules_are_kept_and_not_changed() -> None:
    mine = {"kind": "Exe", "id": "mine", "matching_strategy": "Equals"}
    config = {"ignore_rules": [mine]}
    merged = with_ignored_apps(config)
    assert merged["ignore_rules"][0] == mine
    assert config == {"ignore_rules": [mine]}


def test_a_window_off_every_display_is_hidden(tmp_path: Path) -> None:
    agent = FakeAgent([window("away", 5000, 5000, 400, 300)])
    assert platform(agent, tmp_path).hidden(Client("away"))


@pytest.mark.parametrize(
    "where",
    [
        (1279, 799, 600, 400),  # the corner the daemon hides a window in: one pixel shows
        (-599, 799, 600, 400),  # the other corner
        (1275, 100, 600, 400),  # a window that scrolled away: a 5 px strip
    ],
)
def test_a_window_with_only_a_sliver_on_the_display_is_hidden(
    where: tuple[int, int, int, int], tmp_path: Path
) -> None:
    agent = FakeAgent([window("parked", *where)])
    assert platform(agent, tmp_path).hidden(Client("parked"))


def test_a_window_cut_to_more_than_a_sliver_is_in_view(tmp_path: Path) -> None:
    agent = FakeAgent([window("cut", 1200, 100, 600, 400)])
    assert not platform(agent, tmp_path).hidden(Client("cut"))


def test_a_window_on_the_display_is_not_hidden(tmp_path: Path) -> None:
    agent = FakeAgent([window("here", 100, 100, 400, 300)])
    assert not platform(agent, tmp_path).hidden(Client("here"))


def test_a_window_the_user_minimized_is_not_hidden(tmp_path: Path) -> None:
    agent = FakeAgent([window("min", 5000, 5000, 400, 300, minimized=True)])
    assert not platform(agent, tmp_path).hidden(Client("min"))


def test_a_window_that_does_not_exist_is_not_hidden(tmp_path: Path) -> None:
    assert not platform(FakeAgent(), tmp_path).hidden(Client("none"))


def test_the_frame_is_the_one_the_agent_gives(tmp_path: Path) -> None:
    agent = FakeAgent([window("w", 10, 40, 625, 333)])
    seen = platform(agent, tmp_path).os_window(Client("w"))
    assert seen is not None and (seen.frame.x, seen.frame.y, seen.frame.w, seen.frame.h) == (
        10, 40, 625, 333,
    )


def test_the_work_area_leaves_out_the_menu_bar(tmp_path: Path) -> None:
    area = platform(FakeAgent(), tmp_path).work_area()
    assert area.y == 30 and area.w == 1280


@pytest.mark.parametrize("name", ["../up", "a/../b", "", "/abs"])
def test_a_temp_path_cannot_leave_its_folder(name: str, tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        platform(FakeAgent(), tmp_path).temp_path(name)


def test_a_temp_path_is_in_the_temp_folder(tmp_path: Path) -> None:
    path = platform(FakeAgent(), tmp_path).temp_path("sub/file.json")
    assert path.endswith("/tmp/sub/file.json")


def test_what_a_guest_cannot_do_is_not_offered(tmp_path: Path) -> None:
    p = platform(FakeAgent(), tmp_path)
    assert not p.has("pointer") and not p.has("lock")
    assert p.resizable_display() is None
    with pytest.raises(NotImplementedError):
        p.add_display()


def test_every_scenario_the_runner_names_exists() -> None:
    named = run.WITH_CLIENTS | set(run.UNSUPPORTED) | set(run.MORE)
    for name in sorted(named):
        # `unknown_monitor` is a step of `commands`, as the Windows runner has it
        module = "commands" if name == "unknown_monitor" else name
        importlib.import_module(f"komorebi_vm.core.scenarios.{module}")


def test_the_extra_steps_of_a_scenario_exist() -> None:
    for name, extra in run.MORE.items():
        module = importlib.import_module(f"komorebi_vm.core.scenarios.{name}")
        for step in extra:
            assert callable(getattr(module, step)), f"{name}.{step}"


def test_a_scenario_is_not_both_supported_and_not() -> None:
    assert not set(run.UNSUPPORTED) & set(run.WITH_CLIENTS)


def test_every_scenario_with_marks_exists_and_has_a_reason_for_each() -> None:
    for scenario, marks in run.KNOWN_CHECKS.items():
        importlib.import_module(f"komorebi_vm.core.scenarios.{scenario}")
        assert marks and all(reason.strip() for reason in marks.values()), scenario


def test_a_scenario_is_not_both_marked_and_skipped() -> None:
    assert not set(run.KNOWN_CHECKS) & set(run.UNSUPPORTED)
