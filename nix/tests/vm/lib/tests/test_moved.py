"""The scenarios that moved out of the KDE extras, against a daemon that is right and ones that
are wrong in one way."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import handover, title_rule
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
