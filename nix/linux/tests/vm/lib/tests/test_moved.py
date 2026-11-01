"""The scenarios that moved out of the KDE extras, against a daemon that is right and ones that
are wrong in one way."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.scenarios import (
    commands,
    display_change,
    fullscreen,
    handover,
    hotplug,
    lifecycle_hidden,
    maximizing,
    monocle,
    placement,
    queries,
    resize,
    rules,
    title_rule,
)
from tests.fake_platform import FakePlatform


def opened(platform: FakePlatform, checks: Checks):  # noqa: ANN201
    from komorebi_vm.core.scenarios import tiling

    return tiling.run(platform, checks)


def test_a_daemon_that_does_not_look_again_at_a_renamed_window_passes_title_rule() -> None:
    platform = FakePlatform()
    checks = Checks()
    opened(platform, checks)
    base = checks.passed
    title_rule.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 5


def test_a_daemon_that_manages_an_ignored_title_fails_title_rule() -> None:
    platform = FakePlatform(honours_ignore=False)
    checks = Checks()
    opened(platform, checks)
    title_rule.run(platform, checks)
    assert any("left alone" in f for f in checks.failures)


def test_a_daemon_that_looks_again_at_a_renamed_window_fails_title_rule() -> None:
    platform = FakePlatform(reevaluates_on_rename=True)
    checks = Checks()
    opened(platform, checks)
    title_rule.run(platform, checks)
    assert any("leaves it alone" in f for f in checks.failures)


def test_a_daemon_that_lets_go_and_takes_up_again_passes_handover() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    handover.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 6


def test_a_daemon_that_keeps_an_unmanaged_window_fails_handover() -> None:
    platform = FakePlatform(unmanages=False)
    checks = Checks()
    clients = opened(platform, checks)
    handover.run(platform, checks, clients)
    assert any("unmanage" in f for f in checks.failures)


def test_a_daemon_that_does_not_manage_again_fails_handover() -> None:
    platform = FakePlatform(manages=False)
    checks = Checks()
    clients = opened(platform, checks)
    handover.run(platform, checks, clients)
    assert any("manage: " in f for f in checks.failures)


def test_every_layout_is_tried_with_fewer_windows_and_the_windows_are_opened_again() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    commands.layouts(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == len(commands.LAYOUTS) * 3 + 1
    assert platform.state().managed() == len(clients)


def test_a_layout_that_overlaps_two_windows_fails_commands_layouts() -> None:
    class Overlapping(FakePlatform):
        def komorebic(self, *args: str) -> str:
            out = super().komorebic(*args)
            if args == ("change-layout", "grid"):
                self.tiles = [Rect(0, 0, 1000, 1080)] * len(self.containers)
            return out

    platform = Overlapping()
    checks = Checks()
    clients = opened(platform, checks)
    commands.layouts(platform, checks, clients)
    assert any("grid with 2 windows" in f for f in checks.failures), checks.failures


def test_a_daemon_that_answers_queries_and_loads_resizes_passes_queries() -> None:
    platform = FakePlatform()
    checks = Checks()
    opened(platform, checks)
    base = checks.passed
    queries.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 1 + 7 * 3 + 2
    assert platform.state().workspace().layout == "Columns"


def test_a_daemon_that_answers_the_same_for_every_query_fails_queries() -> None:
    platform = FakePlatform(answers_queries=False)
    checks = Checks()
    opened(platform, checks)
    queries.run(platform, checks)
    assert any("query focused-workspace-layout" in f for f in checks.failures)
    assert any("query focused-container-kind" in f for f in checks.failures)


def test_a_daemon_that_does_not_load_a_resize_fails_queries() -> None:
    platform = FakePlatform(loads_resize=False)
    checks = Checks()
    opened(platform, checks)
    queries.run(platform, checks)
    assert any("quick-save-resize: a resize is undone" in f for f in checks.failures)
    assert any("save-resize: a resize is undone" in f for f in checks.failures)


def test_a_daemon_that_ignores_the_class_of_a_placement_rule_fails_placement() -> None:
    platform = FakePlatform(matches_class=False)
    checks = Checks()
    placement.run(platform, checks)
    assert [f for f in checks.failures if "by the class" in f], checks.failures
    assert checks.failed == 1


def test_a_daemon_that_sends_a_window_back_on_the_next_event_passes_rules() -> None:
    platform = FakePlatform()
    checks = Checks()
    rules.run(platform, checks)
    assert checks.failed == 0, checks.failures


def test_a_daemon_that_only_applies_rules_when_asked_fails_rules() -> None:
    platform = FakePlatform(applies_rules_on_event=False)
    checks = Checks()
    rules.run(platform, checks)
    assert any("at the next event" in f for f in checks.failures), checks.failures


def test_a_daemon_that_keeps_its_rules_when_cleared_fails_rules() -> None:
    platform = FakePlatform(clears_rules=False)
    checks = Checks()
    rules.run(platform, checks)
    assert any("global state has none" in f for f in checks.failures), checks.failures


def test_a_daemon_that_cycles_a_monocle_only_when_told_passes_monocle_behaviour() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    monocle.focus_behaviour(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 4
    assert platform.monocle is None and not platform.monocle_cycles


def test_a_daemon_that_never_cycles_a_monocle_fails_monocle_behaviour() -> None:
    platform = FakePlatform(cycles_in_monocle=False)
    checks = Checks()
    clients = opened(platform, checks)
    monocle.focus_behaviour(platform, checks, clients)
    assert any("with the cycle behaviour" in f for f in checks.failures), checks.failures


def test_a_daemon_that_cycles_a_monocle_with_no_op_fails_monocle_behaviour() -> None:
    platform = FakePlatform(no_op_in_monocle=False)
    checks = Checks()
    clients = opened(platform, checks)
    monocle.focus_behaviour(platform, checks, clients)
    assert any("with the no-op behaviour" in f for f in checks.failures), checks.failures


def test_a_daemon_that_follows_a_window_moved_to_the_other_display_passes() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    hotplug.user_move(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 2


def test_a_daemon_that_ignores_a_window_moved_to_the_other_display_fails() -> None:
    platform = FakePlatform(follows_user_moves=False)
    checks = Checks()
    clients = opened(platform, checks)
    hotplug.user_move(platform, checks, clients)
    assert any("the daemon has the window on the other monitor" in f for f in checks.failures)


def test_a_daemon_that_follows_a_mode_change_passes_display_change() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    display_change.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 4
    assert platform.displays()[1].w == 1920


def test_a_platform_with_no_resizable_display_skips_display_change() -> None:
    platform = FakePlatform(resizable=False)
    checks = Checks()
    clients = opened(platform, checks)
    display_change.run(platform, checks, clients)
    assert checks.failed == 0 and len(checks.skipped) == 1, checks.skipped


def test_a_daemon_that_does_not_follow_a_mode_change_fails_display_change() -> None:
    platform = FakePlatform(follows_modes=False)
    checks = Checks()
    clients = opened(platform, checks)
    display_change.run(platform, checks, clients)
    assert any("the daemon's monitor follows" in f for f in checks.failures)


def test_a_daemon_that_follows_the_pointer_passes_the_edge_cases() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    resize.edge_cases(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 4


def test_a_daemon_that_ignores_the_pointer_fails_the_edge_cases() -> None:
    platform = FakePlatform(follows_pointer=False)
    checks = Checks()
    clients = opened(platform, checks)
    resize.edge_cases(platform, checks, clients)
    assert checks.failed == 4, checks.failures


def test_a_daemon_that_moves_the_other_edges_fails_the_edge_cases() -> None:
    class Sloppy(FakePlatform):
        def resize(self, start: tuple[int, int], dx: int) -> None:
            super().resize(start, dx)
            # the third tile is moved too, though no edge of it was dragged
            a = self.tiles[2]
            self.tiles[2] = Rect(a.x + dx, a.y, a.w, a.h) if dx else a

    platform = Sloppy()
    checks = Checks()
    clients = opened(platform, checks)
    resize.edge_cases(platform, checks, clients)
    assert any("outside the work area" in f or "overlap" in f for f in checks.failures)


def test_a_platform_with_no_pointer_skips_the_edge_cases() -> None:
    platform = FakePlatform(pointer=False)
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    resize.edge_cases(platform, checks, clients)
    assert (checks.passed, checks.failed) == (base, 0)


def test_a_daemon_that_leaves_a_fullscreen_window_alone_passes_fullscreen() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    fullscreen.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 6
    assert platform.state().managed() == len(clients)


def test_a_daemon_that_takes_a_window_out_of_fullscreen_fails_fullscreen() -> None:
    platform = FakePlatform(keeps_fullscreen=False)
    checks = Checks()
    clients = opened(platform, checks)
    fullscreen.run(platform, checks, clients)
    assert any("does not take the window out of fullscreen" in f for f in checks.failures)


def test_a_daemon_that_takes_up_a_maximize_passes_user_maximize() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    maximizing.user(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 12


def test_a_daemon_that_obeys_commands_when_maximized_fails_user_maximize() -> None:
    platform = FakePlatform(ignores_commands_when_maximized=False)
    checks = Checks()
    clients = opened(platform, checks)
    platform.activate(clients[-1])
    maximizing.user(platform, checks, clients)
    assert any("what is asked of the layout does nothing" in f for f in checks.failures)


def test_a_daemon_that_leaves_nothing_hidden_when_it_crashes_passes() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    lifecycle_hidden.crash(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 7
    assert platform.monocle is None


def test_a_crash_that_leaves_a_window_hidden_fails() -> None:
    platform = FakePlatform(leaves_hidden_on_crash=True)
    checks = Checks()
    clients = opened(platform, checks)
    lifecycle_hidden.crash(platform, checks, clients)
    assert any("none is left out of sight" in f for f in checks.failures), checks.failures


def test_a_window_that_is_alone_on_its_workspace_fails_handover_on_the_precondition() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    platform.komorebic("send-to-workspace", "1")
    platform.komorebic("send-to-workspace", "1")
    handover.run(platform, checks, clients)
    assert any("two tiled containers" in f for f in checks.failures)
    assert not any("unmanage" in f for f in checks.failures)
