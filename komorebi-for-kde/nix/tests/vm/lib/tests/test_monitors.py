"""The `monitors` scenario against a daemon that is right, and against ones that are wrong in one
way each."""

import pytest

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import monitors
from tests.fake_desk import FakeDesk


def run_monitors(desk: FakeDesk) -> Checks:
    checks = Checks()
    monitors.run(desk, checks)
    return checks


def test_a_daemon_that_is_right_passes_monitors() -> None:
    desk = FakeDesk(displays=2)
    checks = run_monitors(desk)
    assert checks.failed == 0, checks.failures
    assert checks.passed > 60
    assert desk.display_count == 2
    assert desk.order == []


def test_a_platform_with_one_display_gets_a_second_and_gives_it_back() -> None:
    desk = FakeDesk(displays=1)
    checks = run_monitors(desk)
    assert checks.failed == 0, checks.failures
    assert desk.display_count == 1
    assert len(desk.state().monitors) == 1


def test_other_windows_stay_where_they_are() -> None:
    desk = FakeDesk(displays=1)
    other = desk.spawn("someone else")
    checks = run_monitors(desk)
    assert checks.failed == 0, checks.failures
    assert desk.state().workspace_of(other.title) == 0


@pytest.mark.parametrize(
    ("bug", "what"),
    [
        ("send_follows", "send-to-monitor"),
        ("move_stays", "move-to-monitor"),
        ("send_ignores_workspace", "send-to-monitor-workspace"),
        ("frame_stuck", "tiled inside their own display"),
        ("focus_ignored", "focus-monitor"),
        ("cycle_stuck", "cycle-monitor"),
        ("workspace_stays", "cycle-move-workspace-to-monitor"),
    ],
)
def test_a_daemon_that_is_wrong_fails_monitors(bug: str, what: str) -> None:
    checks = run_monitors(FakeDesk(displays=2, bug=bug))
    assert any(what in f for f in checks.failures), (bug, checks.failures)


def test_a_daemon_that_ignores_the_cursor_fails_at_cursor() -> None:
    checks = Checks()
    monitors.at_cursor(FakeDesk(displays=2, bug="cursor_ignored"), checks)
    assert any("focus-monitor-at-cursor" in f for f in checks.failures), checks.failures


def test_a_daemon_that_follows_the_cursor_passes_at_cursor() -> None:
    desk = FakeDesk(displays=1)
    checks = Checks()
    monitors.at_cursor(desk, checks)
    assert checks.failed == 0, checks.failures
    assert checks.passed == 3
    assert desk.display_count == 1


def test_without_a_pointer_at_cursor_is_skipped() -> None:
    checks = Checks()
    monitors.at_cursor(FakeDesk(displays=1, pointer=False), checks)
    assert checks.passed == 0 and checks.failed == 0
    assert len(checks.skipped) == 1
