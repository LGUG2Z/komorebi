import pytest

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.suite import platform_only, run_suite, shared


def test_checks_are_labelled_with_the_scope_they_are_made_in(
    capsys: pytest.CaptureFixture[str],
) -> None:
    checks = Checks()
    checks.check("plain", True)
    with checks.scoped("kde"):
        checks.check("labelled", True)
        checks.check("bad", False)
    checks.check("plain again", True)
    out = capsys.readouterr().out
    assert "  ok   plain\n" in out and "  ok   [kde] labelled\n" in out
    assert "  ok   plain again\n" in out
    assert checks.failures == ["[kde] bad"]


def test_scopes_nest_and_the_outer_one_comes_back() -> None:
    checks = Checks()
    with checks.scoped("a"):
        with checks.scoped("b"):
            assert checks.scope == "b"
        assert checks.scope == "a"
    assert checks.scope == ""


def test_the_scope_comes_back_when_the_block_raises() -> None:
    checks = Checks()
    with pytest.raises(RuntimeError), checks.scoped("kde"):
        raise RuntimeError
    assert checks.scope == ""


def test_skips_and_known_failures_carry_the_scope() -> None:
    checks = Checks()
    with checks.scoped("windows"):
        checks.skip("x", "no pointer")
        checks.known_failure("y", False, "why")
    assert checks.skipped == ["[windows] x (no pointer)"]
    assert checks.known == ["[windows] y (why)"]


def test_a_subclass_that_overrides_check_still_gets_the_scope() -> None:
    class Recorded(Checks):
        def __init__(self) -> None:
            super().__init__()
            self.seen: list[tuple[str, str]] = []

        def check(self, description, condition):  # noqa: ANN001, ANN201
            self.seen.append((self.scope, description))
            return super().check(description, condition)

    checks = Recorded()
    run_suite(checks, shared(lambda c: c.check("one", True), checks))
    assert checks.seen == [("shared", "one")]


def test_a_suite_runs_its_steps_in_order_each_in_its_own_scope() -> None:
    checks = Checks()
    order: list[str] = []

    def scenario(name: str) -> None:
        order.append(f"{checks.scope}:{name}")

    run_suite(
        checks,
        platform_only("kde", scenario, "first"),
        shared(scenario, "second"),
        platform_only("kde", scenario, "third"),
    )
    assert order == ["kde:first", "shared:second", "kde:third"]
    assert checks.scope == ""
