"""The same scene, as each port prints it, reads as the same Snapshot.

The fixtures are written from the `Serialize` code of the two ports (kfde `komorebi/src/window.rs`,
kfw `komorebi/src/window.rs`), not recorded; a recorded state of each replaces them once a run on
each port exists.
"""

from typing import Any

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.snapshot import parse


def ring(*elements: Any, focused: int = 0) -> dict[str, Any]:
    return {"elements": list(elements), "focused": focused}


def rect(x: int, y: int, w: int, h: int) -> dict[str, int]:
    return {"left": x, "top": y, "right": w, "bottom": h}


def kfde_window(id: int, title: str, where: dict[str, int]) -> dict[str, Any]:
    details = {"title": title, "exe": "foot", "class": "foot", "path": "/usr/bin/foot"}
    return {"id": id, "rect": where, "details": details}


def kfw_window(id: int, title: str, where: dict[str, int]) -> dict[str, Any]:
    return {"hwnd": id, "title": title, "exe": "foot", "class": "foot", "rect": where}


def scene(
    window: Any, monitor_extra: dict[str, Any], state_extra: dict[str, Any]
) -> dict[str, Any]:
    left, right = rect(0, 0, 960, 1080), rect(960, 0, 960, 1080)
    workspace = {
        "name": "main",
        "containers": ring(
            {"id": "a", "locked": False, "windows": ring(window(1, "win1", left))},
            {"id": "b", "locked": False, "windows": ring(window(2, "win2", right))},
            focused=1,
        ),
        "monocle_container": None,
        "maximized_window": None,
        "floating_windows": ring(window(3, "float", rect(10, 10, 300, 200))),
        "layout": {"Default": "BSP"},
        "tile": True,
        "latest_layout": [left, right],
    }
    monitor = {
        "id": 7,
        "size": rect(0, 0, 1920, 1080),
        "work_area_size": rect(0, 0, 1920, 1040),
        "workspaces": ring(workspace),
        **monitor_extra,
    }
    return {"monitors": ring(monitor), "is_paused": False, **state_extra}


KFDE = scene(kfde_window, {"serial_number_id": "x"}, {})
KFW = scene(kfw_window, {"device_id": "d", "name": "DISPLAY1"}, {"has_pending_raise_op": False})


def test_both_ports_read_as_the_same_snapshot() -> None:
    assert parse(KFDE) == parse(KFW)


def test_a_snapshot_names_the_windows_of_each_container() -> None:
    snap = parse(KFDE)
    assert snap.containers() == [["win1"], ["win2"]]
    assert snap.focused_title() == "win2"
    assert snap.managed() == 3


def test_a_window_has_its_frame_and_what_identifies_it() -> None:
    window = parse(KFW).workspace().containers[0][0]
    assert (window.id, window.exe, window.window_class) == (1, "foot", "foot")
    assert window.rect == Rect(0, 0, 960, 1080)


def test_the_layout_is_its_name() -> None:
    snap = parse(KFDE)
    assert snap.workspace().layout == "BSP"
    assert snap.workspace().latest_layout == [Rect(0, 0, 960, 1080), Rect(960, 0, 960, 1080)]


def test_what_only_one_port_has_is_kept_aside() -> None:
    assert parse(KFW).extra == {"has_pending_raise_op": False}
    assert parse(KFDE).extra == {}


def test_a_monocle_container_lists_its_windows() -> None:
    state = scene(kfde_window, {}, {})
    workspace = state["monitors"]["elements"][0]["workspaces"]["elements"][0]
    workspace["monocle_container"] = {
        "id": "m",
        "locked": False,
        "windows": ring(kfde_window(1, "win1", rect(0, 0, 1920, 1040))),
    }
    assert [w.title for w in parse(state).workspace().monocle or []] == ["win1"]


def test_the_focused_window_of_a_stack_is_the_one_the_ring_names() -> None:
    state = scene(kfde_window, {}, {})
    workspace = state["monitors"]["elements"][0]["workspaces"]["elements"][0]
    stack = workspace["containers"]["elements"][1]["windows"]
    stack["elements"].append(kfde_window(4, "under", rect(960, 0, 960, 1080)))
    stack["focused"] = 1
    assert parse(state).focused_title() == "under"
