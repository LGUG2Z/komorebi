"""KDE as a platform, against a machine that answers from a script."""

from itertools import count

import pytest

import komorebi_vm.kde as kde_module
from komorebi_vm import Kde, Rect
from komorebi_vm.kde_platform import KdePlatform

from .conftest import FakeMachine


@pytest.fixture(autouse=True)
def probes(monkeypatch: pytest.MonkeyPatch) -> None:
    """The first probe a test makes is `work-area-1`."""
    monkeypatch.setattr(kde_module, "_PROBES", count(1))


def kde_with_area(machine: FakeMachine, area: str) -> KdePlatform:
    machine.reply("loadScript", output="i 7\n")
    machine.reply("journalctl", output=f"{area}KS:work-area-1 done\n")
    return KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))


def test_the_work_area_is_what_kwin_says_and_not_the_whole_output(machine: FakeMachine) -> None:
    platform = kde_with_area(machine, "KS:work-area-1 area 0 0 1920 1040\n")

    assert platform.work_area() == Rect(0, 0, 1920, 1040)


def test_the_work_area_is_asked_of_the_first_output_less_its_panels(machine: FakeMachine) -> None:
    platform = kde_with_area(machine, "KS:work-area-1 area 0 0 1920 1040\n")

    platform.work_area()

    scripts = [c for c in machine.commands if "KWin.PlacementArea" in c]
    assert scripts, "the script that asks KWin for its placement area was never written"
    assert "currentDesktopForScreen" in scripts[0]


def test_the_work_area_is_the_whole_output_where_kwin_does_not_answer(
    machine: FakeMachine,
) -> None:
    machine.reply("loadScript", output="i 7\n")
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))

    assert platform.work_area() == Rect(0, 0, 1920, 1080)


def test_the_work_area_is_the_whole_output_where_kwin_says_nonsense(machine: FakeMachine) -> None:
    platform = kde_with_area(machine, "KS:work-area-1 area 0 0 many 1040\n")

    assert platform.work_area() == Rect(0, 0, 1920, 1080)
