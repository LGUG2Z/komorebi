"""KDE as a platform, against a machine that answers from a script."""

from itertools import count

import pytest

import komorebi_vm.kde as kde_module
from komorebi_vm import Kde, Rect
from komorebi_vm.core.platform import Client
from komorebi_vm.kde_platform import KdePlatform

from .conftest import FakeMachine


@pytest.fixture(autouse=True)
def probes(monkeypatch: pytest.MonkeyPatch) -> None:
    """The first probe a test makes is `screens-1`."""
    monkeypatch.setattr(kde_module, "_PROBES", count(1))


def kde_with_area(machine: FakeMachine, area: str) -> KdePlatform:
    machine.reply("loadScript", output="i 7\n")
    machine.reply("journalctl", output=f"{area}KS:screens-1 done\n")
    return KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))


def test_the_work_area_is_what_kwin_says_and_not_the_whole_output(machine: FakeMachine) -> None:
    platform = kde_with_area(machine, "KS:screens-1 screen 0 0 1920 1080 0 0 1920 1040\n")

    assert platform.work_area() == Rect(0, 0, 1920, 1040)


def test_the_work_area_is_asked_of_the_first_output_less_its_panels(machine: FakeMachine) -> None:
    platform = kde_with_area(machine, "KS:screens-1 screen 0 0 1920 1080 0 0 1920 1040\n")

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
    platform = kde_with_area(machine, "KS:screens-1 screen 0 0 1920 1080 0 0 many 1040\n")

    assert platform.work_area() == Rect(0, 0, 1920, 1080)


def test_a_window_that_never_appears_fails_the_spawn(machine: FakeMachine) -> None:
    """A scenario that waits for a window to stay unmanaged would pass for one that is not there."""
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))

    with pytest.raises(AssertionError, match="no window titled 'ghost'"):
        platform.spawn("ghost")


def test_a_window_that_appears_is_a_client(machine: FakeMachine) -> None:
    machine.reply("kdotool search", output="{abc}\n")
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))

    assert platform.spawn("here").title == "here"


@pytest.mark.parametrize("name", ["a b", "x;y", "../x", "", "e2e.exe"])
def test_a_program_name_a_port_would_reject_is_rejected(machine: FakeMachine, name: str) -> None:
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))

    with pytest.raises(ValueError, match="not a program name"):
        platform.spawn("w", exe=name)


@pytest.mark.parametrize("act", ["close", "minimize", "activate", "move"])
def test_a_command_for_a_window_that_is_not_there_fails(machine: FakeMachine, act: str) -> None:
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))
    client = Client("gone")

    with pytest.raises(AssertionError, match="no window titled 'gone'"):
        if act == "move":
            platform.move_window(client, 1, 2)
        else:
            getattr(platform, act)(client)
    assert not any("kdotool window" in c for c in machine.commands)


def test_the_displays_are_what_kwin_says_and_not_the_size_they_were_started_with(
    machine: FakeMachine,
) -> None:
    platform = kde_with_area(
        machine,
        "KS:screens-1 screen 1920 0 1280 800 1920 0 1280 800\n"
        "KS:screens-1 screen 0 0 1920 1080 0 0 1920 1040\n",
    )

    assert platform.displays() == [Rect(0, 0, 1920, 1080), Rect(1920, 0, 1280, 800)]


def test_the_work_area_is_that_of_the_leftmost_output(machine: FakeMachine) -> None:
    platform = kde_with_area(
        machine,
        "KS:screens-1 screen 1920 0 1280 800 1920 0 1280 800\n"
        "KS:screens-1 screen 0 0 1920 1080 0 0 1920 1040\n",
    )

    assert platform.work_area() == Rect(0, 0, 1920, 1040)


def test_the_displays_are_side_by_side_at_the_size_given_where_kwin_does_not_say(
    machine: FakeMachine,
) -> None:
    machine.reply("loadScript", output="i 7\n")
    machine.reply("kscreen-doctor -j", output="Virtual-0\nVirtual-1\n")
    platform = KdePlatform(Kde(machine, "main", scale=0.01), (1920, 1080))

    assert platform.displays() == [Rect(0, 0, 1920, 1080), Rect(1920, 0, 1920, 1080)]
