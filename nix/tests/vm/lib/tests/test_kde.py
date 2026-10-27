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


def test_session_attaches_to_the_users_bus_and_the_display_the_manager_reports(
    machine: FakeMachine,
) -> None:
    machine.reply("show-environment", output="PATH=/bin\nWAYLAND_DISPLAY=wayland-1\n")
    machine.reply("[k]splashqml", status=1)

    kde = Kde.session(machine, timeout=0)

    assert "WAYLAND_DISPLAY=wayland-1" in kde.env
    assert "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus" in kde.env


def test_session_waits_for_the_splash_screen_to_go(machine: FakeMachine) -> None:
    machine.reply("show-environment", output="WAYLAND_DISPLAY=wayland-1\n")
    machine.reply("[k]splashqml", status=0)

    with pytest.raises(AssertionError, match="splash"):
        Kde.session(machine, timeout=0)


def test_session_fails_when_the_shell_never_starts(machine: FakeMachine) -> None:
    machine.reply("[p]lasmashell", status=1)

    with pytest.raises(AssertionError, match="plasmashell"):
        Kde.session(machine, timeout=0)


def test_shot_returns_the_screenshot_as_rgb(machine: FakeMachine) -> None:
    image = Kde(machine, "main").shot("desktop")

    assert image.size == (4, 4)
    assert image.getpixel((0, 0)) == (10, 20, 30)
    assert (machine.out_dir / "desktop.png").exists()


def test_start_komorebi_writes_the_arguments_and_starts_the_unit(machine: FakeMachine) -> None:
    Kde(machine, "main").start_komorebi("--log-level", "debug")

    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert "echo KOMOREBI_ARGS='--log-level debug' > /run/user/1000/komorebi-main.env" in scripts
    assert "systemctl --user start komorebi@main.service" in scripts


def test_the_log_is_the_file_of_the_session_and_empty_before_a_start(
    machine: FakeMachine,
) -> None:
    machine.reply("cat /run/user/1000/komorebi-main.log", output="a line\n")
    assert Kde(machine, "main").komorebi_log() == "a line\n"

    machine.replies.clear()
    machine.reply("cat /run/user/1000/komorebi-main.log", status=1, output="")
    assert Kde(machine, "main").komorebi_log() == ""


def test_komorebi_running_asks_the_user_manager(machine: FakeMachine) -> None:
    machine.reply("is-active --quiet komorebi@main.service", status=3)
    assert not Kde(machine, "main").komorebi_running()

    machine.replies.clear()
    assert Kde(machine, "main").komorebi_running()


def test_script_loaded_reads_the_answer_of_kwin(machine: FakeMachine) -> None:
    machine.reply("isScriptLoaded s komorebi-bridge", output="b true\n")
    assert Kde(machine, "main").script_loaded()

    machine.replies.clear()
    machine.reply("isScriptLoaded s komorebi-bridge", output="b false\n")
    assert not Kde(machine, "main").script_loaded()


def test_window_actions_go_through_kdotool_inside_the_session(machine: FakeMachine) -> None:
    kde = Kde(machine, "main")

    kde.activate("{a}")
    kde.minimize("{a}")
    kde.close("{a}")

    assert [inner(command) for command in machine.commands] == [
        "kdotool windowactivate '{a}'",
        "kdotool windowminimize '{a}'",
        "kdotool windowclose '{a}'",
    ]
    assert all("WAYLAND_DISPLAY=wayland-main" in command for command in machine.commands)


def test_wid_by_class_searches_the_class(machine: FakeMachine) -> None:
    machine.reply("kdotool search --class", output="{x}\n")

    assert Kde(machine, "main").wid_by_class("XTerm") == "{x}"
    assert inner(machine.commands[0]) == "kdotool search --class '^XTerm$' | head -1"


def test_headless_with_xwayland_starts_it_and_puts_the_x11_display_in_the_session(
    machine: FakeMachine,
) -> None:
    machine.reply("busctl", status=0)
    machine.reply("/tmp/.X11-unix", output="X1\n")

    kde = Kde.headless(machine, "main", xwayland=True)

    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert any("--output-count 1 --xwayland'" in script for script in scripts)
    assert "DISPLAY=:1" in kde.env.split()
    assert not any(word.startswith("DISPLAY=") for word in Kde(machine, "other").env.split())


def test_monitor_bridge_records_the_calls_to_the_bridge_and_the_replies(
    machine: FakeMachine,
) -> None:
    Kde(machine, "main").monitor_bridge("/tmp/bus.log")

    (script,) = [inner(command) for command in machine.commands]
    assert script.startswith("setsid nohup dbus-monitor --session ")
    assert "\"type='method_call',interface='org.komorebi.Bridge'\"" in script
    assert "\"type='method_return'\"" in script
    assert script.endswith("> /tmp/bus.log 2>&1 < /dev/null &")


def test_spawn_sends_the_output_of_the_client_where_it_is_asked(machine: FakeMachine) -> None:
    kde = Kde(machine, "main")

    kde.spawn("foot")
    kde.spawn("xterm -T x", log="/tmp/x.log")

    assert [inner(command) for command in machine.commands] == [
        "setsid nohup foot > /dev/null 2>&1 < /dev/null &",
        "setsid nohup xterm -T x > /tmp/x.log 2>&1 < /dev/null &",
    ]


def test_a_session_has_the_users_data_and_config_directories_not_roots(
    machine: FakeMachine,
) -> None:
    words = Kde(machine, "main").env.split()

    assert "HOME=/home/komorebi" in words
    assert (
        "XDG_DATA_DIRS=/etc/profiles/per-user/komorebi/share:/run/current-system/sw/share" in words
    )
    assert not any("/root" in word for word in words)
