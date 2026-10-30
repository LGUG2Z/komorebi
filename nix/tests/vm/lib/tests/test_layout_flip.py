"""The `layout_flip` scenario against a daemon that is right, and against ones that are wrong in
one way each."""

import pytest

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.scenarios import layout_flip
from tests.fake_desk import FakeDesk


def run_flip(desk: FakeDesk) -> Checks:
    checks = Checks()
    layout_flip.run(desk, checks)
    return checks


def test_a_daemon_that_flips_from_the_config_passes_layout_flip() -> None:
    checks = run_flip(FakeDesk())
    assert checks.failed == 0, checks.failures
    assert checks.passed > 50


@pytest.mark.parametrize(
    ("bug", "what"),
    [
        ("ignores_option", "on the side of the work area"),
        ("swaps_axes", "on the side of the work area"),
        ("command_sets", "turns it off"),
        ("option_on_all", "workspace 1 has its windows in the order of the layout"),
    ],
)
def test_a_daemon_that_is_wrong_fails_layout_flip(bug: str, what: str) -> None:
    checks = run_flip(FakeDesk(bug=bug))
    assert any(what in f for f in checks.failures), (bug, checks.failures)
