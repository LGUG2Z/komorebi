"""The checks a runner keeps, and the marks of the checks that are known failures of a port."""

from komorebi_vm.runner import RecordedChecks

MARKS = {"covers the display": "the daemon has no fullscreen"}


def test_a_marked_check_that_fails_is_a_known_failure_and_not_a_failure() -> None:
    checks = RecordedChecks(MARKS)
    checks.bad("the window covers the display")
    assert checks.failed == 0
    assert checks.known == ["the window covers the display (the daemon has no fullscreen)"]
    assert checks.results == [
        {
            "check": "the window covers the display",
            "known_failure": True,
            "reason": "the daemon has no fullscreen",
        }
    ]


def test_a_check_that_is_not_marked_fails() -> None:
    checks = RecordedChecks(MARKS)
    checks.bad("another check")
    assert checks.failed == 1 and checks.known == []


def test_no_marks_is_how_it_was() -> None:
    checks = RecordedChecks()
    checks.bad("the window covers the display")
    assert checks.failed == 1 and checks.known == []


def test_a_marked_check_that_passes_says_its_mark_can_go() -> None:
    checks = RecordedChecks(MARKS)
    checks.ok("the window covers the display")
    assert checks.failed == 0
    checks.stale_marks()
    assert checks.failed == 1
    assert "covers the display" in checks.failures[0]


def test_a_mark_whose_check_never_ran_is_not_stale() -> None:
    checks = RecordedChecks(MARKS)
    checks.ok("something else")
    checks.stale_marks()
    assert checks.failed == 0


def test_a_mark_is_said_stale_once() -> None:
    checks = RecordedChecks(MARKS)
    checks.ok("the window covers the display")
    checks.ok("the second window covers the display")
    checks.stale_marks()
    assert checks.failed == 1


def test_the_reason_of_a_step_is_kept_over_a_mark() -> None:
    checks = RecordedChecks(MARKS)
    checks.expected = "the step is known"
    checks.bad("the window covers the display")
    assert checks.known == ["the window covers the display (the step is known)"]
