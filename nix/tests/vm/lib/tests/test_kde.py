import shlex

import pytest

from komorebi_vm import Kde, Rect

from .conftest import FakeMachine


def inner(command: str) -> str:
    """The script a wrapped command hands to `sh -c`, with one level of quoting removed."""
    return shlex.split(command)[-1]


def test_commands_run_as_the_user_inside_the_named_session(machine: FakeMachine) -> None:
    kde = Kde(machine, "main")

    kde.sh("kdotool getactivewindow")

    (command,) = machine.commands
    assert command.startswith("runuser -u komorebi -- env ")
    assert "WAYLAND_DISPLAY=wayland-main" in command
    assert "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/session-main.bus" in command
    assert "XDG_RUNTIME_DIR=/run/user/1000" in command


def test_two_sessions_do_not_share_a_socket(machine: FakeMachine) -> None:
    assert "wayland-a" in Kde(machine, "a").env
    assert "wayland-b" in Kde(machine, "b").env
    assert "wayland-a" not in Kde(machine, "b").env


def test_as_user_has_no_session_in_its_environment(machine: FakeMachine) -> None:
    Kde(machine, "main").as_user("systemctl --user is-system-running")

    (command,) = machine.commands
    assert "WAYLAND_DISPLAY" not in command
    assert "XDG_RUNTIME_DIR=/run/user/1000" in command


def test_headless_writes_the_arguments_and_starts_the_units(machine: FakeMachine) -> None:
    machine.reply("busctl", status=0)

    Kde.headless(machine, "main", size=(1280, 720), outputs=2)

    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert (
        "echo KWIN_ARGS='--width 1280 --height 720 --output-count 2'"
        " > /run/user/1000/kwin-virtual-main.env"
    ) in scripts
    assert "systemctl --user start session-bus@main.service kwin-virtual@main.service" in scripts


def test_headless_reports_the_journal_when_kwin_does_not_come_up(machine: FakeMachine) -> None:
    machine.reply("busctl", status=1)
    machine.reply("journalctl", output="kwin: no GPU")

    with pytest.raises(AssertionError, match="kwin: no GPU"):
        Kde.headless(machine, "main", scale=0.01)


def test_wait_for_scales_its_timeout(machine: FakeMachine) -> None:
    kde = Kde(machine, "main", scale=0.0)

    assert not kde.wait_for(lambda: False, timeout=100)
    assert kde.wait_for(lambda: True, timeout=0)


def test_rect_parses_what_kdotool_prints(machine: FakeMachine) -> None:
    machine.reply(
        "getwindowgeometry", output="Window {abc}\n  Position: 10,20\n  Geometry: 300x400\n"
    )

    assert Kde(machine, "main").rect("{abc}") == Rect(10, 20, 300, 400)


def test_rect_fails_on_output_it_does_not_understand(machine: FakeMachine) -> None:
    machine.reply("getwindowgeometry", output="nothing useful")

    with pytest.raises(AssertionError, match="no geometry"):
        Kde(machine, "main").rect("{abc}")


def test_overlap() -> None:
    assert Rect(0, 0, 10, 10).overlaps(Rect(5, 5, 10, 10))
    assert not Rect(0, 0, 10, 10).overlaps(Rect(10, 0, 10, 10))
    assert not Rect(0, 0, 10, 10).overlaps(Rect(0, 10, 10, 10))


def test_wid_is_the_first_match(machine: FakeMachine) -> None:
    machine.reply("kdotool search", output="{one}\n")

    assert Kde(machine, "main").wid("alpha") == "{one}"


def test_active_is_the_last_line(machine: FakeMachine) -> None:
    machine.reply("getactivewindow", output="noise\n{two}\n")

    assert Kde(machine, "main").active() == "{two}"
    machine.replies.clear()
    machine.reply("getactivewindow", output="")
    assert Kde(machine, "main").active() == ""
