from komorebi_vm import daemon
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
