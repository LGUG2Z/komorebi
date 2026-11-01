"""The parser on what each port printed, recorded from a real run.

`fixtures/kfde-*.json` are the `komorebic state` of kfde in a headless KWin (the `core-e2e` suite
writes them into its output); `fixtures/kfw-*.json` are the `komorebic state` of kfw's daemon on a
Windows 11 guest. Both are raw stdout, unedited: they hold only the test windows. What differs
between the two ports is what `Snapshot` exists to hide, so the checks below are the same for both.
"""

import json
from pathlib import Path

import pytest

from komorebi_vm.core.snapshot import Snapshot, parse

FIXTURES = Path(__file__).parent / "fixtures"
# a terminal cell, in pixels, which is how far a `foot` window can be short of the frame asked for
CELL = 24

# the port, and the titles of the three windows its scenario opened
PORTS = {"kfde": {"neutral1", "neutral2", "neutral3"}, "kfw": {"win1", "win2", "win3"}}


def recorded(name: str) -> Snapshot:
    return parse(json.loads((FIXTURES / f"{name}.json").read_text()))


@pytest.mark.parametrize("port", PORTS)
def test_three_tiled_windows_are_three_containers_of_one(port: str) -> None:
    snap = recorded(f"{port}-tiled")
    assert sorted(t for c in snap.containers() for t in c) == sorted(PORTS[port])
    assert [len(c) for c in snap.containers()] == [1, 1, 1]
    assert snap.managed() == 3
    assert snap.workspace().layout == "BSP"
    assert snap.workspace().tile


@pytest.mark.parametrize("port", PORTS)
def test_the_layout_has_a_rectangle_for_each_container_inside_the_work_area(port: str) -> None:
    snap = recorded(f"{port}-tiled")
    layout = snap.workspace().latest_layout
    assert len(layout) == 3
    area = snap.monitors[0].work_area
    assert all(rect.inside(area) for rect in layout)
    assert not any(a.overlaps(b) for i, a in enumerate(layout) for b in layout[i + 1 :])


@pytest.mark.parametrize("port", PORTS)
def test_each_window_is_where_its_containers_rectangle_is_up_to_the_clients_rounding(
    port: str,
) -> None:
    """The same place, and the size no more than a terminal cell short: kfde's test windows are
    `foot`, which sizes itself to whole cells (a frame of 924 by 504 for a layout of 925 by 505),
    where kfw's test windows have no granularity."""
    snap = recorded(f"{port}-tiled")
    workspace = snap.workspace()
    for container, rect in zip(workspace.containers, workspace.latest_layout, strict=True):
        frame = container[0].rect
        assert frame is not None
        assert (frame.x, frame.y) == (rect.x, rect.y)
        assert 0 <= rect.w - frame.w < CELL
        assert 0 <= rect.h - frame.h < CELL


@pytest.mark.parametrize("port", PORTS)
def test_the_focused_window_is_one_of_the_three(port: str) -> None:
    assert recorded(f"{port}-tiled").focused_title() in PORTS[port]


@pytest.mark.parametrize("port", PORTS)
def test_a_stack_is_a_container_of_two_and_its_focused_window_is_the_one_the_ring_names(
    port: str,
) -> None:
    snap = recorded(f"{port}-stack")
    assert sorted(len(c) for c in snap.containers()) == [1, 2]
    stacked = next(c for c in snap.containers() if len(c) == 2)
    assert snap.focused_title() in stacked
    assert len(snap.workspace().latest_layout) == 2
    assert snap.managed() == 3


def test_a_floating_window_is_not_in_a_container() -> None:
    snap = recorded("kfw-float")
    workspace = snap.workspace()
    assert [w.title for w in workspace.floating] == ["win3"]
    assert sorted(t for c in snap.containers() for t in c) == ["win1", "win2"]
    assert snap.managed() == 3


def test_what_only_kfw_has_is_kept_aside() -> None:
    assert "has_pending_raise_op" in recorded("kfw-tiled").extra
    assert "has_pending_raise_op" not in recorded("kfde-tiled").extra
