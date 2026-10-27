from komorebi_vm import Rect, daemon
from komorebi_vm.daemon import Event

LOG = """\
2026-10-27T12:00:00.000001Z  INFO komorebi::kwin: loaded the KWin bridge script (script id 0)
2026-10-27T12:00:01.000001Z DEBUG komorebi::kwin::state: KWin activated window 2 (alpha)
2026-10-27T12:00:01.000002Z  INFO handle_event: komorebi::process_event: processing event: \
Show for window 2 with notification WindowAdded
2026-10-27T12:00:01.000003Z  INFO handle_event: komorebi::process_event: processing event: \
FocusChange for window 2 with notification WindowActivated
2026-10-27T12:00:02.000001Z DEBUG komorebi::kwin::state: KWin activated window 1 (a (b) c)
"""


def test_events_are_read_in_the_order_they_were_handled() -> None:
    assert daemon.events(LOG) == [
        Event("Show", 2, "WindowAdded"),
        Event("FocusChange", 2, "WindowActivated"),
    ]


def test_window_ids_are_found_by_title_also_one_with_parentheses() -> None:
    assert daemon.window_ids(LOG) == {"alpha": 2, "a (b) c": 1}


def test_an_empty_log_has_nothing_in_it() -> None:
    assert daemon.events("") == []
    assert daemon.window_ids("") == {}


def test_requests_are_the_last_frame_asked_for_each_window() -> None:
    log = (
        "DEBUG komorebi::kwin: asking for window 2 to be at "
        "Rect { left: 10, top: 10, right: 1900, bottom: 1060 }\n"
        "DEBUG komorebi::kwin: asking for window 3 to be at "
        "Rect { left: -5, top: 0, right: 940, bottom: 1060 }\n"
        "DEBUG komorebi::kwin: asking for window 2 to be at "
        "Rect { left: 10, top: 10, right: 940, bottom: 1060 }\n"
    )

    assert daemon.requests(log) == {2: Rect(10, 10, 940, 1060), 3: Rect(-5, 0, 940, 1060)}


def test_asked_finds_the_frame_by_the_title_of_the_window() -> None:
    log = LOG + (
        "DEBUG komorebi::kwin: asking for window 2 to be at "
        "Rect { left: 10, top: 10, right: 940, bottom: 1060 }\n"
    )

    assert daemon.asked(log, "alpha") == Rect(10, 10, 940, 1060)
    assert daemon.asked(log, "a (b) c") is None
    assert daemon.asked(log, "nobody") is None
