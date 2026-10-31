import json

import pytest

from komorebi_vm import latency
from komorebi_vm.latency import Message, Round


def call(time: float, body: dict[str, object]) -> str:
    return (
        f"method call time={time:.6f} sender=:1.5 -> destination=org.komorebi.Bridge serial=1"
        " path=/org/komorebi/Bridge; interface=org.komorebi.Bridge; member=Event\n"
        f'   string "{json.dumps(body)}"\n'
    )


def answer(time: float, cmds: list[dict[str, object]]) -> str:
    batches = [{"batch": 1, "cmds": cmds}]
    return (
        f"method return time={time:.6f} sender=:1.3 -> destination=:1.5 serial=9 reply_serial=2\n"
        f'   string "{json.dumps(batches)}"\n'
    )


def geo(uuid: str, x: int) -> dict[str, object]:
    return {"op": "geo", "id": uuid, "x": x, "y": 0, "w": 100, "h": 100}


def frame(time: float, uuid: str) -> str:
    return call(time, {"type": "geometry", "id": uuid, "geo": {"x": 0, "y": 0, "w": 9, "h": 9}})


def test_messages_are_parsed_with_their_time_member_and_string() -> None:
    text = call(1.5, {"type": "added"}) + (
        "method return time=2.000000 sender=:1.1 -> destination=:1.2 serial=3 reply_serial=1\n"
        "   boolean true\n"
    )

    assert latency.messages(text) == [
        Message("method call", 1.5, "Event", '{"type": "added"}'),
        Message("method return", 2.0, "", ""),
    ]


def test_a_round_lasts_until_the_last_window_it_moved_reports_its_frame() -> None:
    text = (
        answer(10.0, [geo("a", 0), geo("b", 100)])
        + frame(10.004, "a")
        + frame(10.010, "b")
        + frame(10.016, "a")
    )

    (only,) = latency.rounds(text)
    assert (only.sent, only.windows) == (10.0, 2)
    assert only.seconds == pytest.approx(0.016)


def test_asking_again_for_the_same_frames_is_not_a_round() -> None:
    text = (
        answer(10.0, [geo("a", 0)])
        + frame(10.01, "a")
        + answer(11.0, [geo("a", 0), {"op": "activate", "id": "a"}])
        + answer(12.0, [geo("a", 50)])
        + frame(12.02, "a")
    )

    rounds = latency.rounds(text)
    assert [r.sent for r in rounds] == [10.0, 12.0]
    assert (rounds[1].sent, rounds[1].windows) == (12.0, 1)
    assert rounds[1].seconds == pytest.approx(0.02)


def test_a_window_that_never_reports_leaves_its_round_unsettled() -> None:
    text = answer(10.0, [geo("a", 0)]) + frame(13.0, "a")

    assert latency.rounds(text) == [Round(10.0, 1, None)]


def test_a_round_ends_where_the_next_one_starts() -> None:
    text = answer(10.0, [geo("a", 0)]) + answer(10.5, [geo("a", 9)]) + frame(10.6, "a")

    assert [r.seconds for r in latency.rounds(text)] == [None, pytest.approx(0.1)]


def test_percentile_is_the_nearest_rank() -> None:
    values = [float(n) for n in range(1, 21)]

    assert latency.percentile(values, 0.5) == 10.0
    assert latency.percentile(values, 0.95) == 19.0
    assert latency.percentile(values, 1.0) == 20.0
    assert latency.percentile([3.0], 0.5) == 3.0
