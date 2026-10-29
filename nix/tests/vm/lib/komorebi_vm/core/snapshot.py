"""The daemon's state, in one shape for every port.

`komorebic state` prints a different JSON for each port. kfde serializes a window as
`{id, rect, details: {title, exe, class, path}}`, kfw as `{hwnd, title, exe, class, rect}`; a
monitor has `serial_number_id` on one and `device_id` on the other; and the state has fields only
one of them has. A scenario must not care, so it reads a
[`Snapshot`](komorebi_vm.core.snapshot.Snapshot): monitors, workspaces, containers and windows,
with the same names and types whatever printed them.

`rect` is `{left, top, right, bottom}` on both ports, where `right` and `bottom` are the width and
the height, which is what [`Rect`](komorebi_vm.core.geometry.Rect) takes.
"""

from dataclasses import dataclass, field
from typing import Any

from komorebi_vm.core.geometry import Rect


@dataclass(frozen=True)
class Window:
    """A window the daemon manages."""

    id: int
    """The daemon's id: the compositor's on kfde, the handle on kfw."""
    title: str
    exe: str
    window_class: str
    rect: Rect | None
    """Where the daemon last saw the window. `None` when the state has no rect for it."""


@dataclass(frozen=True)
class Workspace:
    """A workspace of a monitor."""

    name: str | None
    layout: str
    """The layout's name: `BSP`, `Columns`, ... or `Custom`."""
    tile: bool
    containers: list[list[Window]]
    """The tiled containers, in order, each with its stack of windows."""
    focused_container: int
    focused_windows: list[int]
    """For each container, the place in it of the window that has the focus (a stack's top)."""
    monocle: list[Window] | None
    """The windows of the monocle container, or `None` when the workspace is not in monocle."""
    maximized: Window | None
    floating: list[Window]
    latest_layout: list[Rect]
    """The rectangles the layout last computed, one for each container."""

    def windows(self) -> list[Window]:
        """Every window of the workspace: the tiled ones, the monocle ones, the maximized one and
        the floating ones."""
        found = [w for container in self.containers for w in container]
        found += self.monocle or []
        found += [self.maximized] if self.maximized is not None else []
        return found + self.floating


@dataclass(frozen=True)
class Monitor:
    """A monitor, with its workspaces."""

    id: int
    size: Rect
    work_area: Rect
    workspaces: list[Workspace]
    focused_workspace: int


@dataclass(frozen=True)
class Snapshot:
    """The state of a daemon: its monitors, and which is focused."""

    monitors: list[Monitor]
    focused_monitor: int
    paused: bool = False
    extra: dict[str, Any] = field(default_factory=dict, compare=False)
    """What only this port's state has, such as `has_pending_raise_op` on kfw."""

    def workspace(self, workspace: int | None = None, monitor: int = 0) -> Workspace:
        """A workspace of a monitor, by position; the focused one when none is given."""
        m = self.monitors[monitor]
        return m.workspaces[m.focused_workspace if workspace is None else workspace]

    def workspace_of(self, title: str, monitor: int = 0) -> int | None:
        """The place of the workspace of a monitor that has a window with this title, or `None`."""
        for i, workspace in enumerate(self.monitors[monitor].workspaces):
            if any(w.title == title for w in workspace.windows()):
                return i
        return None

    def containers(self, workspace: int | None = None, monitor: int = 0) -> list[list[str]]:
        """The titles of the windows of each tiled container of a workspace."""
        return [[w.title for w in c] for c in self.workspace(workspace, monitor).containers]

    def managed(self) -> int:
        """The number of windows the daemon has, on every workspace of every monitor."""
        return sum(len(ws.windows()) for m in self.monitors for ws in m.workspaces)

    def focused_title(self, workspace: int | None = None, monitor: int = 0) -> str:
        """The title of the focused window of a workspace (the focused one when none is given): the
        focused window of its focused container. Empty if the workspace has no container."""
        ws = self.workspace(workspace, monitor)
        if not ws.containers:
            return ""
        index = min(ws.focused_container, len(ws.containers) - 1)
        container = ws.containers[index]
        if not container:
            return ""
        return container[min(ws.focused_windows[index], len(container) - 1)].title


def _ring(value: dict[str, Any]) -> list[Any]:
    return list(value["elements"])


def _focused(value: dict[str, Any]) -> int:
    return int(value.get("focused", 0))


def _rect(value: dict[str, Any] | None) -> Rect | None:
    if value is None:
        return None
    return Rect(value["left"], value["top"], value["right"], value["bottom"])


def _window(value: dict[str, Any]) -> Window:
    # kfde nests what it knows about the window under `details`; kfw puts it beside the handle
    details = value.get("details", value)
    return Window(
        id=int(value["id"] if "id" in value else value["hwnd"]),
        title=str(details.get("title", "")),
        exe=str(details.get("exe", "")),
        window_class=str(details.get("class", "")),
        rect=_rect(value.get("rect")),
    )


def _layout(value: Any) -> str:
    # `{"Default": "BSP"}` for a built-in layout, `{"Custom": [...]}` for a custom one
    if isinstance(value, dict) and value:
        name, inner = next(iter(value.items()))
        return str(inner) if name == "Default" else str(name)
    return str(value)


def _workspace(value: dict[str, Any]) -> Workspace:
    containers = [[_window(w) for w in _ring(c["windows"])] for c in _ring(value["containers"])]
    monocle = value.get("monocle_container")
    maximized = value.get("maximized_window")
    return Workspace(
        name=value.get("name"),
        layout=_layout(value.get("layout")),
        tile=bool(value.get("tile", True)),
        containers=containers,
        focused_container=_focused(value["containers"]),
        focused_windows=[_focused(c["windows"]) for c in _ring(value["containers"])],
        monocle=[_window(w) for w in _ring(monocle["windows"])] if monocle else None,
        maximized=_window(maximized) if maximized else None,
        floating=[_window(w) for w in _ring(value["floating_windows"])],
        latest_layout=[r for r in map(_rect, value.get("latest_layout", [])) if r is not None],
    )


def _monitor(value: dict[str, Any]) -> Monitor:
    size = _rect(value["size"])
    work_area = _rect(value.get("work_area_size")) or size
    assert size is not None and work_area is not None
    return Monitor(
        id=int(value["id"]),
        size=size,
        work_area=work_area,
        workspaces=[_workspace(w) for w in _ring(value["workspaces"])],
        focused_workspace=_focused(value["workspaces"]),
    )


_COMMON = {"monitors", "is_paused"}


def parse(state: dict[str, Any]) -> Snapshot:
    """Reads the JSON of `komorebic state`, from either port."""
    monitors = state["monitors"]
    return Snapshot(
        monitors=[_monitor(m) for m in _ring(monitors)],
        focused_monitor=_focused(monitors),
        paused=bool(state.get("is_paused", False)),
        extra={k: v for k, v in state.items() if k not in _COMMON},
    )
