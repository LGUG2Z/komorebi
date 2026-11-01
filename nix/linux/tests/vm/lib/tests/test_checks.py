import pytest

from komorebi_vm.core.checks import Checks


def test_counts_passes_and_failures(capsys: pytest.CaptureFixture[str]) -> None:
    checks = Checks()

    assert checks.check("a holds", True)
    assert not checks.check("b does not hold", False)

    assert (checks.passed, checks.failed) == (1, 1)
    out = capsys.readouterr().out
    assert "  ok   a holds" in out
    assert "  FAIL b does not hold" in out


def test_a_callable_condition_is_called() -> None:
    checks = Checks()

    assert checks.check("lazy", lambda: 1 + 1 == 2)


def test_finish_raises_only_when_something_failed(capsys: pytest.CaptureFixture[str]) -> None:
    checks = Checks()
    checks.check("fine", True)
    checks.finish()
    assert "passed: 1 failed: 0" in capsys.readouterr().out

    checks.check("broken", False)
    with pytest.raises(AssertionError, match="^1 checks failed: broken$"):
        checks.finish()
    assert "passed: 1 failed: 1" in capsys.readouterr().out


def test_a_known_failure_is_listed_and_does_not_fail(capsys: pytest.CaptureFixture[str]) -> None:
    checks = Checks()
    assert checks.known_failure("x", False, "why") is False
    checks.finish()
    out = capsys.readouterr().out
    assert "passed: 0 failed: 0" in out and "known failures: 1" in out
    assert checks.known == ["x (why)"]


def test_a_known_failure_that_holds_fails_so_the_marker_goes() -> None:
    checks = Checks()
    checks.known_failure("x", True, "why")
    assert checks.failed == 1 and "no longer a known failure" in checks.failures[0]
