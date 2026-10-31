"""The focus and moving scenarios against a daemon that is right, and against ones that are not."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow
from komorebi_vm.core.scenarios import (
    closing,
    commands,
    config_reload,
    flipping,
    floating,
    focus,
    focus_follows_mouse,
    hotplug,
    layouts,
    lifecycle,
    lifecycle_hidden,
    locking,
    manage_rules,
    maximizing,
    minimizing,
    monocle,
    moving,
    padding,
    pausing,
    placement,
    promoting,
    resize,
    resize_commands,
    rules,
    rules_class,
    rules_exe,
    scrolling,
    stacks,
    startup_rules,
    state_fidelity,
    tiling,
    tiling_off,
    wallpaper,
    workspace_moves,
    workspaces,
)
from tests.fake_platform import FakePlatform
from tests.fake_strip import FakeStrip
from tests.fake_wallpaper import FakeWallpaper


def opened(platform: FakePlatform, checks: Checks):  # noqa: ANN201
    return tiling.run(platform, checks)


def test_a_daemon_that_is_right_passes_both() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    focus.run(platform, checks, clients)
    moving.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 10


def test_a_move_that_moves_nothing_fails() -> None:
    platform = FakePlatform(moves=False)
    checks = Checks()
    clients = opened(platform, checks)
    moving.run(platform, checks, clients)
    assert any("swapped tiles" in f for f in checks.failures)


def test_a_move_that_loses_the_focus_fails() -> None:
    platform = FakePlatform(focus_follows_move=False)
    checks = Checks()
    clients = opened(platform, checks)
    moving.run(platform, checks, clients)
    assert any("keeps the focus" in f for f in checks.failures)


def test_a_window_that_is_gone_is_not_in_view_for_workspaces() -> None:
    """`hidden` is false for a window the system does not have, which is not one that is shown."""

    class Vanishes(FakePlatform):
        def os_window(self, client: Client) -> OsWindow | None:
            return None if client.title == "win1" else super().os_window(client)

        def hidden(self, client: Client) -> bool:
            return False if client.title == "win1" else super().hidden(client)

    platform = Vanishes()
    checks = Checks()
    clients = opened(platform, checks)
    workspaces.run(platform, checks, clients)
    assert any("in view" in f for f in checks.failures)


def test_a_daemon_that_is_right_passes_stacks() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    stacks.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 10


def test_a_daemon_that_is_right_passes_workspaces() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    workspaces.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 7


def test_a_daemon_that_follows_the_pointer_passes_resize() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    resize.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 8


def test_a_daemon_that_ignores_the_pointer_fails_resize() -> None:
    platform = FakePlatform(follows_pointer=False)
    checks = Checks()
    clients = opened(platform, checks)
    resize.run(platform, checks, clients)
    assert any("pixels wider" in f for f in checks.failures)


def test_a_platform_with_no_pointer_skips_resize() -> None:
    platform = FakePlatform(pointer=False)
    checks = Checks()
    clients = opened(platform, checks)
    passed = checks.passed
    resize.run(platform, checks, clients)
    assert (checks.passed, checks.failed) == (passed, 0)


def test_a_daemon_that_is_right_passes_monocle() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    monocle.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 5


def test_a_daemon_that_is_right_passes_closing() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    closing.run(platform, checks, clients, clients[-1])
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_is_right_passes_lifecycle() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    lifecycle.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 5


def test_a_daemon_that_does_not_take_its_windows_up_again_fails_lifecycle() -> None:
    class Forgetful(FakePlatform):
        def start_komorebi(self) -> None:
            self.running = True
            self.spaces = [[], [], []]

    platform = Forgetful()
    checks = Checks()
    clients = opened(platform, checks)
    lifecycle.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_layouts() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    layouts.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > len(layouts.LAYOUTS) * 6


def test_a_layout_that_puts_tiles_over_each_other_fails_layouts() -> None:
    class Overlapping(FakePlatform):
        def komorebic(self, *args: str) -> str:
            out = super().komorebic(*args)
            if args == ("change-layout", "grid"):
                self.tiles = [Rect(0, 0, 1000, 1080)] * len(self.containers)
            return out

    platform = Overlapping()
    checks = Checks()
    clients = opened(platform, checks)
    layouts.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_floating() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    floating.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 6


def test_a_float_that_stays_in_the_tiling_fails_floating() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args == ("toggle-float",) else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    floating.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_minimizing() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    minimizing.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_keeps_the_room_of_a_minimized_window_fails_minimizing() -> None:
    class Keeping(FakePlatform):
        def _relayout(self) -> None:
            pass

    platform = Keeping()
    checks = Checks()
    clients = opened(platform, checks)
    minimizing.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_padding() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    padding.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 5


def test_a_daemon_that_ignores_the_padding_fails_padding() -> None:
    class Ignoring(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args[0].endswith("padding") else super().komorebic(*args)

    platform = Ignoring()
    checks = Checks()
    clients = opened(platform, checks)
    padding.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_maximizing() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    maximizing.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_does_not_maximize_fails_maximizing() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args == ("toggle-maximize",) else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    maximizing.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_workspace_moves() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    workspace_moves.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 6


def test_a_daemon_that_does_not_follow_the_window_fails_workspace_moves() -> None:
    class Staying(FakePlatform):
        def komorebic(self, *args: str) -> str:
            if args[0] == "move-to-workspace":
                return super().komorebic("send-to-workspace", args[1])
            return super().komorebic(*args)

    platform = Staying()
    checks = Checks()
    clients = opened(platform, checks)
    workspace_moves.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_resize_commands() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    resize_commands.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_does_not_resize_fails_resize_commands() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args[0] == "resize-axis" else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    resize_commands.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_decrease_that_gives_back_less_than_the_increase_fails_resize_commands() -> None:
    """Within what a terminal rounds to (24 px), so the window alone cannot tell."""

    class Short(FakePlatform):
        def komorebic(self, *args: str) -> str:
            if args == ("resize-axis", "horizontal", "decrease"):
                i = self.focus
                a, b = self.tiles[i], self.tiles[i + 1]
                self.tiles[i] = Rect(a.x, a.y, a.w - 30, a.h)
                self.tiles[i + 1] = Rect(b.x - 30, b.y, b.w + 30, b.h)
                return ""
            return super().komorebic(*args)

    platform = Short()
    checks = Checks()
    clients = opened(platform, checks)
    resize_commands.run(platform, checks, clients)
    assert any("the layout gives it the width it had" in f for f in checks.failures)


def test_a_daemon_that_is_right_passes_promoting() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    promoting.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_does_not_promote_fails_promoting() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args == ("promote",) else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    promoting.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_flipping() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    flipping.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_does_not_flip_fails_flipping() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args[0] == "flip-layout" else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    flipping.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_pausing() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    pausing.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_manages_while_paused_fails_pausing() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args == ("toggle-pause",) else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    pausing.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_tiling_off() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    tiling_off.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_ignores_toggle_tiling_fails_tiling_off() -> None:
    class Stubborn(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args == ("toggle-tiling",) else super().komorebic(*args)

    platform = Stubborn()
    checks = Checks()
    clients = opened(platform, checks)
    tiling_off.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_lifecycle_hidden() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    lifecycle_hidden.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_loses_the_put_away_window_fails_lifecycle_hidden() -> None:
    class Forgetful(FakePlatform):
        def start_komorebi(self) -> None:
            super().start_komorebi()
            self.spaces = [[c for c in self.spaces[0] if c != ["win3"]], [], []]
            self.unmanaged.append("win3")

    platform = Forgetful()
    checks = Checks()
    clients = opened(platform, checks)
    lifecycle_hidden.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_scrolling() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    scrolling.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 3


def test_a_daemon_that_loses_the_focused_window_off_screen_fails_scrolling() -> None:
    class Off(FakePlatform):
        def os_window(self, client):  # noqa: ANN001, ANN201
            window = super().os_window(client)
            if window is not None and window.focused:
                return OsWindow(window.title, Rect(5000, 0, 100, 100), focused=True)
            return window

    platform = Off()
    checks = Checks()
    clients = opened(platform, checks)
    scrolling.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_rules_class() -> None:
    platform = FakePlatform()
    checks = Checks()
    rules_class.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_ignores_class_rules_fails_rules_class() -> None:
    class Deaf(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if "class" in args else super().komorebic(*args)

    platform = Deaf()
    checks = Checks()
    rules_class.run(platform, checks)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_rules_exe() -> None:
    platform = FakePlatform()
    checks = Checks()
    rules_exe.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_ignores_exe_rules_fails_rules_exe() -> None:
    class Deaf(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if "exe" in args else super().komorebic(*args)

    platform = Deaf()
    checks = Checks()
    rules_exe.run(platform, checks)
    assert checks.failed > 0


def test_a_daemon_that_reloads_the_file_passes_config_reload() -> None:
    platform = FakePlatform()
    checks = Checks()
    config_reload.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 6


def test_a_daemon_that_does_not_reload_the_file_fails_config_reload() -> None:
    platform = FakePlatform(reloads=False)
    checks = Checks()
    config_reload.run(platform, checks)
    assert checks.failed > 0


def test_a_daemon_that_tiles_again_on_unlock_passes_locking() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    locking.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_does_not_tile_again_on_unlock_fails_locking() -> None:
    platform = FakePlatform(retiles_on_unlock=False)
    checks = Checks()
    clients = opened(platform, checks)
    locking.run(platform, checks, clients)
    assert checks.failed > 0


def test_a_daemon_that_is_right_passes_manage_rules() -> None:
    platform = FakePlatform()
    checks = Checks()
    manage_rules.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 8


def test_a_daemon_that_ignores_manage_rules_fails_manage_rules() -> None:
    class Deaf(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args[0] == "manage-rule" else super().komorebic(*args)

    platform = Deaf()
    checks = Checks()
    manage_rules.run(platform, checks)
    assert checks.failed > 0


def test_a_daemon_that_shows_what_a_window_was_given_passes_state_fidelity() -> None:
    platform = FakePlatform()
    checks = Checks()
    state_fidelity.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 4


def test_a_daemon_that_loses_the_class_fails_state_fidelity() -> None:
    class Forgetful(FakePlatform):
        def spawn(self, title, app_id=None, exe=None, rename_to=None, after=0):  # noqa: ANN001, ANN202
            client = super().spawn(title, app_id=None, exe=exe)
            return client

    platform = Forgetful()
    checks = Checks()
    state_fidelity.run(platform, checks)
    assert checks.failed > 0


def test_a_command_the_port_lacks_is_skipped_by_name() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    commands.layouts(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.skipped == []

    commands.sweep(
        platform, checks, clients, config="unused", commands=("focus right", "no-such-command")
    )
    assert checks.failed == 0, checks.failures
    assert [s.split(" (")[0] for s in checks.skipped] == ["komorebic no-such-command"]


def test_a_command_that_breaks_the_daemon_is_a_failure() -> None:
    class Breaks(FakePlatform):
        """After `cycle-focus next` the daemon forgets a window the operating system still has."""

        broken = False

        def komorebic(self, *args: str) -> str:
            self.broken = self.broken or args == ("cycle-focus", "next")
            return super().komorebic(*args)

        def state(self):  # noqa: ANN201
            if not self.broken:
                return super().state()
            forgotten = self.spaces[0].pop()
            try:
                return super().state()
            finally:
                self.spaces[0].append(forgotten)

    platform = Breaks()
    checks = Checks()
    clients = opened(platform, checks)
    commands.sweep(platform, checks, clients, config="unused", commands=("cycle-focus next",))
    assert any("afterwards it manages" in f for f in checks.failures)


def test_the_commands_that_only_print_are_checked_for_their_exit() -> None:
    platform = FakePlatform()
    checks = Checks()
    commands.prints(platform, checks)
    assert checks.failed == 0
    assert checks.skipped == []


def test_a_daemon_that_follows_its_rules_passes_rules() -> None:
    platform = FakePlatform()
    checks = Checks()
    rules.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 7


def test_a_daemon_that_forgets_an_ignore_rule_fails_rules() -> None:
    class Forgets(FakePlatform):
        def komorebic(self, *args: str) -> str:
            return "" if args[:1] == ("ignore-rule",) else super().komorebic(*args)

    platform = Forgets()
    checks = Checks()
    rules.run(platform, checks)
    assert any("ignore rule matches" in f for f in checks.failures)


def test_a_daemon_that_follows_the_displays_passes_hotplug() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    hotplug.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 6


def test_a_platform_with_one_display_gets_a_second_first() -> None:
    platform = FakePlatform()
    platform.display_count = 1
    checks = Checks()
    clients = opened(platform, checks)
    hotplug.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures


def test_a_daemon_that_does_not_see_the_displays_fails_hotplug() -> None:
    platform = FakePlatform()
    platform.forgets_displays = True
    checks = Checks()
    clients = opened(platform, checks)
    hotplug.run(platform, checks, clients)
    assert any("a monitor for each display" in f for f in checks.failures)


def test_the_sweep_plugs_in_a_second_display_when_there_is_only_one() -> None:
    platform = FakePlatform()
    platform.display_count = 1
    checks = Checks()
    clients = opened(platform, checks)
    commands.sweep(platform, checks, clients, config="unused", commands=("focus right",))
    assert platform.display_count == 2
    assert checks.failed == 0, checks.failures


def test_a_daemon_that_loses_a_window_sent_to_no_monitor_fails() -> None:
    class Loses(FakePlatform):
        def komorebic_status(self, *args: str) -> tuple[int, str]:
            if args[:1] == ("send-to-monitor",):
                self.containers[self.focus].clear()
                return 0, ""
            return super().komorebic_status(*args)

    platform = Loses()
    checks = Checks()
    clients = opened(platform, checks)
    commands.unknown_monitor(platform, checks, clients)
    assert any("send-to-monitor 7" in f for f in checks.failures)


def test_a_daemon_that_ignores_a_monitor_that_does_not_exist_passes() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    commands.unknown_monitor(platform, checks, clients)
    assert checks.failed == 0, checks.failures


def test_a_known_failure_does_not_fail_the_suite_until_it_is_fixed() -> None:
    class Loses(FakePlatform):
        def komorebic_status(self, *args: str) -> tuple[int, str]:
            if args[:1] == ("send-to-monitor",):
                self.containers[self.focus].clear()
                return 0, ""
            return super().komorebic_status(*args)

    reason = "the container goes first"
    broken, checks = Loses(), Checks()
    clients = opened(broken, checks)
    commands.unknown_monitor(broken, checks, clients, known_failure=reason)
    assert checks.failed == 0, checks.failures
    assert len(checks.known) >= 1

    fixed, checks = FakePlatform(), Checks()
    clients = opened(fixed, checks)
    commands.unknown_monitor(fixed, checks, clients, known_failure=reason)
    assert checks.failed == 2 and checks.known == []


def test_a_daemon_that_follows_the_placement_rules_passes() -> None:
    platform = FakePlatform()
    checks = Checks()
    placement.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed == len(placement.CASES)


def test_a_daemon_that_ignores_the_placement_rules_fails() -> None:
    class Ignores(FakePlatform):
        def restart_with(self, config: dict) -> None:
            super().restart_with(config)
            self.placement = None

    platform = Ignores()
    checks = Checks()
    placement.run(platform, checks)
    assert checks.failed == len(placement.CASES) - 1, checks.failures


def test_a_case_a_port_cannot_start_is_a_known_failure_and_the_rest_run() -> None:
    number = "by the number of the place, counted from 1"

    class Refuses(FakePlatform):
        def restart_with(self, config: dict) -> None:
            workspace = config["monitors"][0]["workspaces"][0]
            if isinstance(workspace.get("initial_window_placement_rules"), int):
                raise AssertionError("exited at start")
            super().restart_with(config)

    platform = Refuses()
    checks = Checks()
    placement.run(platform, checks, {number: "an integer is not accepted"})
    assert checks.failed == 0, checks.failures
    assert checks.passed == len(placement.CASES) - 1
    assert len(checks.known) == 1 and "exited at start" in checks.known[0]


def test_a_daemon_that_reads_the_rules_of_its_file_passes_startup_rules() -> None:
    platform = FakePlatform()
    checks = Checks()
    startup_rules.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed == 5


def test_a_daemon_that_ignores_the_rules_of_its_file_fails_startup_rules() -> None:
    class Ignores(FakePlatform):
        def restart_with(self, config: dict) -> None:
            self.start_komorebi()

    platform = Ignores()
    checks = Checks()
    startup_rules.run(platform, checks)
    assert checks.failed >= 3, checks.failures


def test_a_daemon_that_follows_the_mouse_passes_focus_follows_mouse() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    focus_follows_mouse.run(platform, checks, clients[0], clients[1])
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 7
    assert not platform.follows_mouse


def test_a_daemon_that_does_not_follow_the_mouse_fails_focus_follows_mouse() -> None:
    platform = FakePlatform()
    platform.mouse_ignored = True
    checks = Checks()
    clients = opened(platform, checks)
    focus_follows_mouse.run(platform, checks, clients[0], clients[1])
    assert checks.failed >= 2, checks.failures


def test_focus_follows_mouse_is_skipped_without_a_pointer() -> None:
    platform = FakePlatform(pointer=False)
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    focus_follows_mouse.run(platform, checks, clients[0], clients[1])
    assert checks.failed == 0 and checks.passed == base


def test_a_daemon_that_tiles_inside_the_work_area_passes_with_a_panel() -> None:
    platform = FakePlatform(panel=40)
    checks = Checks()
    clients = opened(platform, checks)
    for scenario in (monocle, maximizing, layouts, floating, flipping, closing):
        if scenario is closing:
            closing.run(platform, checks, clients, clients[0])
        else:
            scenario.run(platform, checks, clients)
    assert platform.work_area().h == 1040
    assert checks.failed == 0, checks.failures
    assert checks.passed > 20


def test_a_daemon_that_tiles_under_the_panel_fails_tiling() -> None:
    platform = FakePlatform(panel=40, ignores_panel=True)
    checks = Checks()
    opened(platform, checks)
    assert any("inside the work area" in f for f in checks.failures)


def test_a_window_that_is_maximized_past_the_work_area_fails_maximizing() -> None:
    platform = FakePlatform(panel=40, ignores_panel=True)
    checks = Checks()
    clients = [platform.spawn(t) for t in ("a", "b", "c")]
    maximizing.run(platform, checks, clients)
    assert any("without going past the work area" in f for f in checks.failures)


def test_a_window_in_monocle_past_the_work_area_fails_monocle() -> None:
    platform = FakePlatform(panel=40, ignores_panel=True)
    checks = Checks()
    clients = [platform.spawn(t) for t in ("a", "b", "c")]
    monocle.run(platform, checks, clients)
    assert any("without going past the work area" in f for f in checks.failures)


def test_a_second_display_of_another_size_passes_hotplug() -> None:
    platform = FakePlatform(second=Rect(0, 0, 1280, 800))
    checks = Checks()
    clients = opened(platform, checks)
    hotplug.run(platform, checks, clients)
    assert platform.displays()[1] == Rect(1920, 0, 1280, 800)
    assert checks.failed == 0, checks.failures


def test_focus_follows_mouse_is_driven_without_naming_an_implementation() -> None:
    """The implementation is a port's own: the scenario leaves it to the command's default."""

    class Records(FakePlatform):
        def __init__(self) -> None:
            super().__init__()
            self.said: list[tuple[str, ...]] = []

        def komorebic(self, *args: str) -> str:
            self.said.append(args)
            return super().komorebic(*args)

    platform = Records()
    checks = Checks()
    clients = opened(platform, checks)
    focus_follows_mouse.run(platform, checks, clients[0], clients[2])
    assert checks.failed == 0, checks.failures
    assert not [a for a in platform.said if "--implementation" in a]


def test_the_window_is_moved_inside_the_work_area_of_the_platform_while_locked() -> None:
    class Records(FakePlatform):
        def __init__(self) -> None:
            super().__init__(panel=40)
            self.targets: list[tuple[int, int]] = []

        def move_window(self, client: Client, x: int, y: int) -> None:
            self.targets.append((x, y))
            super().move_window(client, x, y)

    platform = Records()
    checks = Checks()
    clients = opened(platform, checks)
    locking.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    (x, y) = platform.targets[0]
    assert platform.work_area().x <= x < platform.work_area().w
    assert platform.work_area().y <= y < platform.work_area().h


def test_a_platform_that_cannot_move_a_window_while_locked_fails_locking() -> None:
    """Else the check that the window is back in its tile after the unlock is a vacuous one."""

    class Cannot(FakePlatform):
        def move_window(self, client: Client, x: int, y: int) -> None:
            pass

    platform = Cannot()
    checks = Checks()
    clients = opened(platform, checks)
    locking.run(platform, checks, clients)
    assert any("was moved while the screen was locked" in f for f in checks.failures)


def test_a_daemon_that_is_right_passes_the_scrolling_columns() -> None:
    platform = FakeStrip()
    checks = Checks()
    scrolling.columns(platform, checks, [])
    assert checks.failed == 0, checks.failures
    assert checks.passed > 15


def test_a_daemon_that_pulls_the_windows_back_fails_the_scrolling_columns() -> None:
    class Pulls(FakeStrip):
        def _strip_frame(self, title):  # noqa: ANN001, ANN202
            r = super()._strip_frame(title)
            return Rect(max(r.x, 0), r.y, r.w, r.h)

    platform = Pulls()
    checks = Checks()
    scrolling.columns(platform, checks, [])
    assert checks.failed > 0


def test_a_strip_with_a_monitor_next_to_it_leaks_nothing() -> None:
    platform = FakeStrip()
    checks = Checks()
    scrolling.two_monitors(platform, checks, [])
    assert checks.failed == 0, checks.failures
    assert checks.passed > 10


def test_a_daemon_that_rests_windows_on_the_next_monitor_fails_two_monitors() -> None:
    platform = FakeStrip()
    platform.leaks = True
    checks = Checks()
    scrolling.two_monitors(platform, checks, [])
    assert checks.failed > 0


def test_a_daemon_that_sets_the_wallpapers_passes_wallpaper() -> None:
    platform = FakeWallpaper()
    checks = Checks()
    wallpaper.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 8


def test_a_daemon_that_leaves_a_changed_wallpaper_fails_wallpaper() -> None:
    platform = FakeWallpaper()
    platform.puts_back = False
    checks = Checks()
    wallpaper.run(platform, checks)
    assert checks.failed > 0
