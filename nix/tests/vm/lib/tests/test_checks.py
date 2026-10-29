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
