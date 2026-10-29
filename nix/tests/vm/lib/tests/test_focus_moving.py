"""The focus and moving scenarios against a daemon that is right, and against ones that are not."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import focus, moving, stacks, tiling, workspaces
from tests.fake_platform import FakePlatform


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
