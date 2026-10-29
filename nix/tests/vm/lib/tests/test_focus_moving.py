"""The focus and moving scenarios against a daemon that is right, and against ones that are not."""

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import (
    closing,
    commands,
    focus,
    hotplug,
    monocle,
    moving,
    resize,
    rules,
    stacks,
    tiling,
    workspaces,
)
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
