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


def test_a_suite_returns_what_its_steps_returned() -> None:
    checks = Checks()
    results = run_suite(checks, shared(lambda: 1), platform_only("kde", lambda: "two"))
    assert results == [1, "two"]


def test_a_step_says_what_it_runs(capsys: pytest.CaptureFixture[str]) -> None:
    def scenario() -> None:
        pass

    run_suite(Checks(), shared(scenario), platform_only("kde", scenario))
    out = capsys.readouterr().out
    assert "== [shared] test_suite.scenario" in out and "== [kde] test_suite.scenario" in out


def test_a_step_prints_how_long_it_took(capsys: pytest.CaptureFixture[str]) -> None:
    def scenario() -> None:
        pass

    run_suite(Checks(), shared(scenario))
    out = capsys.readouterr().out
    assert "   [shared] test_suite.scenario: " in out and " s\n" in out


def test_steps_not_named_in_the_list_are_left_out(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.setenv("E2E_ONLY", "second, nothing")
    ran: list[str] = []

    def first() -> str:
        ran.append("first")
        return "a"

    def second() -> str:
        ran.append("second")
        return "b"

    def setup() -> str:
        ran.append("setup")
        return "c"

    results = run_suite(
        Checks(),
        platform_only("kde", setup, always=True),
        shared(first),
        shared(second),
    )
    assert ran == ["setup", "second"]
    assert results == ["c", None, "b"]
    assert "test_suite.first: left out by E2E_ONLY" in capsys.readouterr().out


def test_with_no_list_every_step_runs(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("E2E_ONLY", raising=False)
    ran: list[int] = []
    run_suite(Checks(), shared(lambda: ran.append(1)), shared(lambda: ran.append(2)))
    assert ran == [1, 2]


def test_the_older_name_of_the_variable_still_works(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.delenv("E2E_ONLY", raising=False)
    monkeypatch.setenv("KFDE_ONLY", "second")
    ran: list[str] = []

    def first() -> None:
        ran.append("first")

    def second() -> None:
        ran.append("second")

    run_suite(Checks(), shared(first), shared(second))
    assert ran == ["second"]
    assert "test_suite.first: left out by KFDE_ONLY" in capsys.readouterr().out


def test_the_new_name_wins_over_the_older_one(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("E2E_ONLY", "second")
    monkeypatch.setenv("KFDE_ONLY", "first")
    ran: list[str] = []

    def first() -> None:
        ran.append("first")

    def second() -> None:
        ran.append("second")

    run_suite(Checks(), shared(first), shared(second))
    assert ran == ["second"]
