"""The scenarios that moved out of the KDE extras, against a daemon that is right and ones that
are wrong in one way."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.scenarios import commands, handover, placement, queries, title_rule
from tests.fake_platform import FakePlatform


def opened(platform: FakePlatform, checks: Checks):  # noqa: ANN201
    from komorebi_vm.core.scenarios import tiling

    return tiling.run(platform, checks)


def test_a_daemon_that_follows_a_rename_passes_title_rule() -> None:
    platform = FakePlatform()
    checks = Checks()
    opened(platform, checks)
    base = checks.passed
    title_rule.run(platform, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 4


def test_a_daemon_that_manages_an_ignored_title_fails_title_rule() -> None:
    platform = FakePlatform(honours_ignore=False)
    checks = Checks()
    opened(platform, checks)
    title_rule.run(platform, checks)
    assert any("left alone" in f for f in checks.failures)


def test_a_daemon_that_misses_the_rename_fails_title_rule() -> None:
    platform = FakePlatform(follows_renames=False)
    checks = Checks()
    opened(platform, checks)
    title_rule.run(platform, checks)
    assert any("it is tiled" in f for f in checks.failures)


def test_a_daemon_that_lets_go_and_takes_up_again_passes_handover() -> None:
    platform = FakePlatform()
    checks = Checks()
    clients = opened(platform, checks)
    base = checks.passed
    handover.run(platform, checks, clients)
    assert checks.failed == 0, checks.failures
    assert checks.passed - base == 5


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
