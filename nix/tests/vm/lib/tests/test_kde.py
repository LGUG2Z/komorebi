import json
import shlex

import pytest

from komorebi_vm import Kde, Outputs, Rect

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


def test_rect_rounds_what_a_fractional_scale_makes_of_a_frame(machine: FakeMachine) -> None:
    machine.reply(
        "getwindowgeometry",
        output="Window {abc}\n  Position: 0.4,10.5\n  Geometry: 695.3333333333334x400.6\n",
    )

    assert Kde(machine, "main").rect("{abc}") == Rect(0, 10, 695, 401)


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
    machine.reply("panels().length", output='s "1\\n"\n')

    kde = Kde.session(machine, timeout=0)

    assert "WAYLAND_DISPLAY=wayland-1" in kde.env
    assert "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus" in kde.env


def test_session_waits_for_the_splash_screen_to_go(machine: FakeMachine) -> None:
    machine.reply("show-environment", output="WAYLAND_DISPLAY=wayland-1\n")
    machine.reply("[k]splashqml", status=0)

    with pytest.raises(AssertionError, match="splash"):
        Kde.session(machine, timeout=0)


def test_session_waits_for_the_panel_to_be_made(machine: FakeMachine) -> None:
    machine.reply("show-environment", output="WAYLAND_DISPLAY=wayland-1\n")
    machine.reply("[k]splashqml", status=1)
    machine.reply("panels().length", output='s "0\\n"\n')

    with pytest.raises(AssertionError, match="panel"):
        Kde.session(machine, timeout=0)


def test_the_panels_are_counted_from_the_shells_answer(machine: FakeMachine) -> None:
    kde = Kde(machine, "main")
    machine.reply("panels().length", output='s "2\\n"\n')
    assert kde.panels() == 2

    machine.replies.clear()
    machine.reply("panels().length", status=1, output="Failed to call method\n")
    assert kde.panels() == 0


def a_state_with_a_work_area(area_bottom: int) -> str:
    """What `komorebic state` prints for a monitor of 1280x800 with a work area this high."""
    monitor = {
        "size": {"left": 0, "top": 0, "right": 1280, "bottom": 800},
        "work_area_size": {"left": 0, "top": 0, "right": 1280, "bottom": area_bottom},
    }
    return json.dumps({"monitors": {"elements": [monitor], "focused": 0}})


def test_the_panel_has_room_when_the_work_area_is_less_than_the_monitor(
    machine: FakeMachine,
) -> None:
    kde = Kde(machine, "main")

    machine.reply("komorebic state", output=a_state_with_a_work_area(738))
    assert kde.panel_has_room()
    assert kde.wait_for_panel_room(timeout=0)


def test_the_panel_has_no_room_while_the_work_area_is_the_whole_monitor(
    machine: FakeMachine,
) -> None:
    machine.reply("komorebic state", output=a_state_with_a_work_area(800))
    kde = Kde(machine, "main", scale=0.0)

    assert not kde.panel_has_room()
    # the wait ends when the time is up, and says the room did not come
    assert not kde.wait_for_panel_room(timeout=100)


def test_a_daemon_that_does_not_answer_properly_has_no_panel_room_and_does_not_fail_the_test(
    machine: FakeMachine,
) -> None:
    kde = Kde(machine, "main", scale=0.0)

    # not running, or too busy to answer in its second: komorebic prints a line with nothing in it
    machine.reply("komorebic state", status=1, output="")
    assert not kde.panel_has_room()
    machine.replies.clear()
    machine.reply("komorebic state", output="\n")
    assert not kde.panel_has_room()
    machine.replies.clear()
    machine.reply("komorebic state", output="{}")
    assert not kde.panel_has_room()


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
    Kde(machine, "main").start_komorebi("--fake-backend")

    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert "echo KOMOREBI_ARGS=--fake-backend > /run/user/1000/komorebi-main.env" in scripts
    assert "systemctl --user start komorebi@main.service" in scripts


def test_the_log_is_the_file_of_the_session_and_empty_before_a_start(
    machine: FakeMachine,
) -> None:
    machine.reply("cat /run/user/1000/komorebi-main.log", output="a line\n")
    assert Kde(machine, "main").komorebi_log() == "a line\n"

    machine.replies.clear()
    machine.reply("cat /run/user/1000/komorebi-main.log", status=1, output="")
    assert Kde(machine, "main").komorebi_log() == ""


def test_kill_komorebi_sends_sigkill_and_waits_for_the_unit_to_be_inactive(
    machine: FakeMachine,
) -> None:
    machine.reply("is-active --quiet komorebi@main.service", status=3)
    Kde(machine, "main").kill_komorebi()

    commands = [inner(command) for command in machine.commands if " sh -c " in command]
    assert "systemctl --user kill --signal=KILL komorebi@main.service" in commands


def test_kill_komorebi_fails_the_test_when_the_daemon_survives(machine: FakeMachine) -> None:
    kde = Kde(machine, "main", scale=0.01)

    with pytest.raises(AssertionError, match="still running"):
        kde.kill_komorebi()


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
    kde.maximize("{a}")
    kde.unmaximize("{a}")
    kde.fullscreen("{a}")
    kde.fullscreen("{a}", on=False)
    kde.close("{a}")

    assert [inner(command) for command in machine.commands] == [
        "kdotool windowactivate '{a}'",
        "kdotool windowminimize '{a}'",
        "kdotool windowstate --add MAXIMIZED_VERT '{a}'",
        "kdotool windowstate --add MAXIMIZED_HORZ '{a}'",
        "kdotool windowstate --remove MAXIMIZED_VERT '{a}'",
        "kdotool windowstate --remove MAXIMIZED_HORZ '{a}'",
        "kdotool windowstate --add FULLSCREEN '{a}'",
        "kdotool windowstate --remove FULLSCREEN '{a}'",
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


def test_restart_kwin_restarts_its_unit_alone_and_waits_for_the_new_one(
    machine: FakeMachine,
) -> None:
    machine.reply("busctl", status=0)

    Kde(machine, "main").restart_kwin()

    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert scripts[0] == "systemctl --user restart kwin-virtual@main.service"
    assert "busctl --user --no-pager list | grep -q '^org.kde.KWin '" in scripts[1:]


def test_restart_kwin_reports_the_journal_when_kwin_does_not_come_back(
    machine: FakeMachine,
) -> None:
    machine.reply("busctl", status=1)
    machine.reply("journalctl", output="kwin: the socket is taken")

    with pytest.raises(AssertionError, match="the socket is taken"):
        Kde(machine, "main", scale=0.01).restart_kwin()


def test_komorebic_runs_inside_the_session_and_returns_what_it_printed(
    machine: FakeMachine,
) -> None:
    machine.reply("komorebic query", output="2\n")

    assert Kde(machine, "main").komorebic("query", "focused-workspace-index") == "2\n"
    (command,) = machine.commands
    assert inner(command) == "komorebic query focused-workspace-index"
    assert "WAYLAND_DISPLAY=wayland-main" in command
    assert "XDG_RUNTIME_DIR=/run/user/1000" in command


def test_komorebic_fails_the_test_when_no_daemon_answers(machine: FakeMachine) -> None:
    machine.reply("komorebic", status=1, output="")

    with pytest.raises(AssertionError, match="komorebic state"):
        Kde(machine, "main").komorebic("state")


def test_a_daemon_that_says_nothing_is_asked_again(
    machine: FakeMachine, monkeypatch: pytest.MonkeyPatch
) -> None:
    kde = Kde(machine, "main", scale=0.0)
    answers = iter(["\n", "\n", a_state(workspace())])

    def execute(command: str, /) -> tuple[int, str]:
        machine.commands.append(command)
        return 0, next(answers)

    monkeypatch.setattr(machine, "execute", execute)

    assert kde.state()["monitors"]["focused"] == 0
    assert len(machine.commands) == 3


def test_a_daemon_that_always_says_nothing_fails_the_test_after_the_attempts(
    machine: FakeMachine,
) -> None:
    machine.reply("komorebic state", output="\n")

    with pytest.raises(AssertionError, match="did not answer"):
        Kde(machine, "main", scale=0.0).state(attempts=3)
    assert len(machine.commands) == 3


def test_a_komorebic_that_gives_up_waiting_is_asked_again(
    machine: FakeMachine, monkeypatch: pytest.MonkeyPatch
) -> None:
    # `komorebic` waits a second for the daemon and panics (exit status 101) when it is not enough
    answers = iter([(101, "thread 'main' panicked"), (0, a_state(workspace()))])

    def execute(command: str, /) -> tuple[int, str]:
        machine.commands.append(command)
        return next(answers)

    monkeypatch.setattr(machine, "execute", execute)

    assert Kde(machine, "main", scale=0.0).state()["monitors"]["focused"] == 0
    assert len(machine.commands) == 2


def test_an_answer_that_is_not_json_fails_at_once(machine: FakeMachine) -> None:
    machine.reply("komorebic state", output="panic: Resource temporarily unavailable\n")

    with pytest.raises(ValueError):
        Kde(machine, "main", scale=0.0).state()
    assert len(machine.commands) == 1


def window(title: str) -> dict[str, object]:
    return {"id": 1, "details": {"title": title, "exe": "foot", "class": "foot", "path": ""}}


def container(*titles: str, focused: int = 0) -> dict[str, object]:
    return {
        "id": "c",
        "locked": False,
        "windows": {"elements": [window(t) for t in titles], "focused": focused},
    }


def workspace(
    *containers: dict[str, object],
    focused: int = 0,
    floating: tuple[str, ...] = (),
    monocle: dict[str, object] | None = None,
    maximized: str | None = None,
) -> dict[str, object]:
    return {
        "containers": {"elements": list(containers), "focused": focused},
        "monocle_container": monocle,
        "maximized_window": None if maximized is None else window(maximized),
        "floating_windows": {"elements": [window(t) for t in floating], "focused": 0},
    }


def a_state(*workspaces: dict[str, object]) -> str:
    """What `komorebic state` prints for one monitor with these workspaces."""
    monitor = {"id": 1, "workspaces": {"elements": list(workspaces), "focused": 0}}
    return json.dumps({"monitors": {"elements": [monitor], "focused": 0}})


def test_the_state_is_what_komorebic_state_prints(machine: FakeMachine) -> None:
    machine.reply("komorebic state", output=a_state(workspace()))

    state = Kde(machine, "main").state()

    assert state["monitors"]["elements"][0]["workspaces"]["focused"] == 0
    assert [inner(command) for command in machine.commands] == ["komorebic state"]


def test_containers_are_the_titles_of_each_container_in_order(machine: FakeMachine) -> None:
    machine.reply(
        "komorebic state",
        output=a_state(
            workspace(container("a"), container("b", "c", focused=1)), workspace(container("d"))
        ),
    )
    kde = Kde(machine, "main")

    assert kde.containers() == [["a"], ["b", "c"]]
    assert kde.containers(1) == [["d"]]


def test_a_workspace_without_containers_has_no_titles(machine: FakeMachine) -> None:
    machine.reply("komorebic state", output=a_state(workspace()))

    assert Kde(machine, "main").containers() == []


def test_monocle_is_the_titles_of_the_monocle_container_and_empty_without_one(
    machine: FakeMachine,
) -> None:
    machine.reply(
        "komorebic state",
        output=a_state(
            workspace(container("a"), monocle=container("m", "n")), workspace(container("d"))
        ),
    )
    kde = Kde(machine, "main")

    assert kde.monocle() == ["m", "n"]
    assert kde.monocle(1) == []


def test_managed_counts_the_windows_of_every_container_of_the_workspace(
    machine: FakeMachine,
) -> None:
    machine.reply(
        "komorebic state",
        output=a_state(workspace(container("a", "b"), container("c")), workspace(container("d"))),
    )
    kde = Kde(machine, "main")

    assert kde.managed() == 3
    assert kde.managed(1) == 1


def test_managed_all_counts_the_windows_the_floating_ones_the_monocle_and_the_maximized_one(
    machine: FakeMachine,
) -> None:
    first = {
        "id": 1,
        "workspaces": {"elements": [workspace(container("a", "b"), floating=("f",))], "focused": 0},
    }
    second = {
        "id": 2,
        "workspaces": {
            "elements": [
                workspace(container("c"), maximized="x"),
                workspace(monocle=container("m")),
            ],
            "focused": 0,
        },
    }
    machine.reply(
        "komorebic state",
        output=json.dumps({"monitors": {"elements": [first, second], "focused": 0}}),
    )

    assert Kde(machine, "main").managed_all() == 6


def test_managed_is_zero_for_a_workspace_without_containers(machine: FakeMachine) -> None:
    machine.reply("komorebic state", output=a_state(workspace()))

    assert Kde(machine, "main").managed() == 0


def test_focused_title_is_the_focused_window_of_the_focused_container(
    machine: FakeMachine,
) -> None:
    machine.reply(
        "komorebic state",
        output=a_state(
            workspace(container("a"), container("b", "c", focused=1), focused=1),
            workspace(container("d")),
        ),
    )
    kde = Kde(machine, "main")

    assert kde.focused_title() == "c"
    assert kde.focused_title(1) == "d"


def test_focused_title_is_empty_for_a_workspace_without_containers(machine: FakeMachine) -> None:
    machine.reply("komorebic state", output=a_state(workspace()))

    assert Kde(machine, "main").focused_title() == ""


def test_desk_is_the_desktop_number_kdotool_prints_or_zero(machine: FakeMachine) -> None:
    machine.reply("{a}", output="2\n")
    machine.reply("{b}", status=1, output="no such window")
    kde = Kde(machine, "main")

    assert kde.desk("{a}") == 2
    assert kde.desk("{b}") == 0


def test_shown_and_desktops_ask_kwin(machine: FakeMachine) -> None:
    machine.reply("kdotool get_desktop", output="3\n")
    machine.reply("VirtualDesktopManager count", output="u 4\n")
    kde = Kde(machine, "main")

    assert kde.shown() == 3
    assert kde.desktops() == 4


def test_desktop_names_are_the_third_of_each_triple_kwin_prints(machine: FakeMachine) -> None:
    machine.reply(
        "VirtualDesktopManager desktops",
        output='a(uss) 3 0 "a1" "Desktop 1" 1 "b2" "Mail and chat" 2 "c3" "komorebi-park"\n',
    )

    assert Kde(machine, "main").desktop_names() == ["Desktop 1", "Mail and chat", "komorebi-park"]


def test_there_are_no_desktop_names_when_kwin_does_not_answer(machine: FakeMachine) -> None:
    machine.reply("VirtualDesktopManager desktops", status=1, output="")

    assert Kde(machine, "main").desktop_names() == []


def test_kwinrc_reads_a_key_of_its_own_group(machine: FakeMachine) -> None:
    machine.reply(
        "cat /home/komorebi/.config/kwinrc",
        output=(
            "[Desktops]\nPerOutputVirtualDesktops=false\n"
            "[Windows]\nPerOutputVirtualDesktops = true\n"
        ),
    )
    kde = Kde(machine, "main")

    assert kde.kwinrc("Windows", "PerOutputVirtualDesktops") == "true"
    assert kde.kwinrc("Windows", "FocusPolicy") is None


def test_kwinrc_is_none_without_a_file(machine: FakeMachine) -> None:
    machine.reply("cat /home/komorebi/.config/kwinrc", status=1)

    assert Kde(machine, "main").kwinrc("Windows", "PerOutputVirtualDesktops") is None


def test_kwinrc_group_is_every_key_of_the_group_and_nothing_of_the_others(
    machine: FakeMachine,
) -> None:
    machine.reply(
        "cat /home/komorebi/.config/kwinrc",
        output="[Round-Corners]\nSize = 10\nOuterOutlineColor=1,2,3\n[Windows]\nSize=99\n",
    )
    kde = Kde(machine, "main")

    assert kde.kwinrc_group("Round-Corners") == {"Size": "10", "OuterOutlineColor": "1,2,3"}
    assert kde.kwinrc_group("Nowhere") == {}


def test_kwinrc_group_is_empty_without_a_file(machine: FakeMachine) -> None:
    machine.reply("cat /home/komorebi/.config/kwinrc", status=1)

    assert Kde(machine, "main").kwinrc_group("Windows") == {}


def test_an_effect_is_supported_or_loaded_when_kwin_says_true(machine: FakeMachine) -> None:
    machine.reply("isEffectSupported s kwin4_effect_shapecorners", output="b true\n")
    machine.reply("isEffectLoaded s kwin4_effect_shapecorners", output="b false\n")
    kde = Kde(machine, "main")

    assert kde.effect_supported("kwin4_effect_shapecorners")
    assert not kde.effect_loaded("kwin4_effect_shapecorners")


def test_an_effect_is_not_supported_when_the_question_fails(machine: FakeMachine) -> None:
    machine.reply("isEffectSupported", status=1, output="Failed to call")

    assert not Kde(machine, "main").effect_supported("kwin4_effect_shapecorners")


def test_unload_effect_asks_kwin_over_the_session_bus(machine: FakeMachine) -> None:
    Kde(machine, "main").unload_effect("kwin4_effect_shapecorners")

    (command,) = machine.commands
    assert "busctl --user call org.kde.KWin /Effects org.kde.kwin.Effects unloadEffect s" in command
    assert "kwin4_effect_shapecorners" in command


def test_a_colored_window_is_a_foot_of_one_color_that_the_session_waits_for(
    machine: FakeMachine,
) -> None:
    machine.reply("kdotool search --name", output="{abc}\n")
    kde = Kde(machine, "main")

    kde.colored_window("red", "ff0000")

    spawn = next(command for command in machine.commands if "setsid nohup" in command)
    assert "foot -T red -o colors-dark.background=ff0000 -o colors-dark.alpha=1.0 sleep 3000" in (
        inner(spawn)
    )
    assert "WAYLAND_DISPLAY=wayland-main" in spawn


def test_close_colored_windows_kills_what_ran_sleep_and_waits_for_them_to_go(
    machine: FakeMachine,
) -> None:
    machine.reply("pgrep", status=1)
    kde = Kde(machine, "main")

    kde.close_colored_windows()

    assert "pkill -f 'sleep 3000'" in inner(machine.commands[0])
    assert "pgrep -f 'sleep 3000'" in inner(machine.commands[1])


def test_a_kwin_script_is_loaded_run_and_unloaded_and_its_reports_returned(
    machine: FakeMachine,
) -> None:
    machine.reply("loadScript", output="i 7\n")
    machine.reply("journalctl", output="other\nKS:probe 2 outputs\nkwin: KS:probe done\n")
    kde = Kde(machine, "main", scale=0.01)

    lines = kde.kwin_script("probe", 'report(workspace.screens.length, "outputs"); report("done");')

    assert lines == ["2 outputs"]
    scripts = [inner(command) for command in machine.commands if " sh -c " in command]
    assert any("/tmp/kwin-script-probe.js" in s and "function report()" in s for s in scripts)
    assert any("/Scripting/Script7 org.kde.kwin.Script run" in s for s in scripts)
    assert any("unloadScript s probe" in s for s in scripts)


def test_a_kwin_script_that_is_kept_stays_loaded(machine: FakeMachine) -> None:
    machine.reply("loadScript", output="i 8\n")
    machine.reply("journalctl", output="KS:listen done\n")
    kde = Kde(machine, "main", scale=0.01)

    kde.kwin_script("listen", 'report("done");', keep=True)

    assert not any("unloadScript" in command for command in machine.commands)


def test_a_kwin_script_that_never_reports_it_is_done_fails_the_test(machine: FakeMachine) -> None:
    machine.reply("loadScript", output="i 9\n")
    kde = Kde(machine, "main", scale=0.01)

    with pytest.raises(AssertionError, match="never reported"):
        kde.kwin_script("silent", "")


def test_a_rect_is_inside_an_area_that_has_every_one_of_its_pixels() -> None:
    area = Rect(0, 0, 1920, 1080)

    assert Rect(0, 0, 1920, 1080).inside(area)
    assert Rect(100, 50, 800, 600).inside(area)
    assert not Rect(-1, 0, 100, 100).inside(area)
    assert not Rect(1900, 0, 100, 100).inside(area)
    assert not Rect(0, 1000, 100, 100).inside(area)
    assert Rect(1920, 0, 800, 600).inside(Rect(1920, 0, 1920, 1080))
    assert Rect(-1, 0, 1921, 1080).inside(area, slack=1)
    assert not Rect(-2, 0, 100, 100).inside(area, slack=1)


def test_outputs_count_the_enabled_ones_when_called(machine: FakeMachine) -> None:
    machine.reply("length", output="2\n")

    assert Kde(machine, "main").outputs() == 2
    assert isinstance(Kde(machine, "main").outputs, Outputs)


def test_outputs_are_named_left_to_right_and_a_disabled_one_keeps_its_name(
    machine: FakeMachine,
) -> None:
    machine.reply("select(.enabled == true)", output="Virtual-0\nVirtual-1\n")
    machine.reply("select(.enabled == false)", output="Virtual-1\n")
    outputs = Kde(machine, "main").outputs

    assert outputs.names() == ["Virtual-0", "Virtual-1"]
    assert outputs.names(enabled=False) == ["Virtual-1"]
    assert all("sort_by(.pos.x)" in inner(command) for command in machine.commands)


def test_an_output_is_disabled_enabled_and_scaled_with_kscreen_doctor(
    machine: FakeMachine,
) -> None:
    outputs = Kde(machine, "main").outputs

    outputs.disable("Virtual-1")
    outputs.enable("Virtual-1")
    outputs.scale("Virtual-0", 1.5)
    outputs.scale("Virtual-0", 1)

    assert [inner(command) for command in machine.commands] == [
        "kscreen-doctor output.Virtual-1.disable",
        "kscreen-doctor output.Virtual-1.enable",
        "kscreen-doctor output.Virtual-0.scale.1.5",
        "kscreen-doctor output.Virtual-0.scale.1",
    ]


def test_spawn_komorebi_runs_the_daemon_in_the_session_with_its_log_where_it_is_read(
    machine: FakeMachine,
) -> None:
    Kde(machine, "session").spawn_komorebi("--keybindings", "/tmp/my skhdrc")

    (command,) = machine.commands
    assert "WAYLAND_DISPLAY=wayland-session" in command
    assert inner(command) == (
        "setsid nohup env NO_COLOR=1 komorebi --keybindings '/tmp/my skhdrc'"
        " > /run/user/1000/komorebi-session.log 2>&1 < /dev/null &"
    )


def test_kill_spawned_komorebi_kills_and_waits_until_it_is_gone(machine: FakeMachine) -> None:
    machine.reply("pgrep -x komorebi", status=1)

    Kde(machine, "main").kill_spawned_komorebi()

    assert machine.commands[:2] == ["pkill -KILL -x komorebi", "pgrep -x komorebi"]


def test_shortcut_actions_are_the_action_names_of_the_component(machine: FakeMachine) -> None:
    answer = {
        "type": "aas",
        "data": [
            [
                ["komorebi", "Alt+L", "komorebi", "Alt+L"],
                ["komorebi", "Alt+H", "komorebi", "Alt+H"],
            ]
        ],
    }
    machine.reply("allActionsForComponent as 4 komorebi", output=json.dumps(answer))
    assert Kde(machine, "main").shortcut_actions() == ["Alt+H", "Alt+L"]

    machine.replies.clear()
    machine.reply("allActionsForComponent", status=1)
    assert Kde(machine, "main").shortcut_actions() == []
