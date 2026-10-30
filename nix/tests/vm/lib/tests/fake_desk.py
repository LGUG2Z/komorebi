"""A desktop with monitors, and a daemon that follows the monitor and flip commands.

Each monitor is a display of its own size, with workspaces of containers that the layout tiles in
the display: columns, rows or BSP, mirrored along the axes the workspace is flipped on. It does
what the real daemon does with `send-to-monitor` and its family, `focus-monitor`, the workspace
moves and `layout_flip`, so that the `monitors` and `layout_flip` scenarios can be run against a
daemon that is right, and against ones that are wrong in one way (`bug`).
"""

from dataclasses import dataclass, field
from typing import Any

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow
from komorebi_vm.core.snapshot import Snapshot, parse

DISPLAYS = [Rect(0, 0, 1920, 1080), Rect(1920, 0, 1280, 720), Rect(3200, 0, 1920, 1080)]
"""The displays, left to right: the second is of another size."""

AXES = {"horizontal": "Horizontal", "vertical": "Vertical", "horizontal-and-vertical": "HV"}
BOTH = "HorizontalAndVertical"


@dataclass
class Space:
    name: str | None = None
    layout: str = "BSP"
    flip: str | None = None
    containers: list[str] = field(default_factory=list)
    focus: int = 0


@dataclass
class Mon:
    spaces: list[Space]
    shown: int = 0


def _toggle(current: str | None, axis: str) -> str | None:
    """What `flip_layout` makes of the flip a workspace has and the axis it is asked to flip on."""
    axis = BOTH if axis == "HV" else axis
    if current is None:
        return axis
    if current == axis:
        return None
    if current == BOTH:
        return "Vertical" if axis == "Horizontal" else "Horizontal"
    return BOTH


def _bsp(i: int, n: int, area: Rect, h: bool, v: bool) -> list[Rect]:
    if n == 1:
        return [area]
    if i % 2 == 0:
        left = Rect(area.x, area.y, area.w // 2, area.h)
        right = Rect(area.x + area.w // 2, area.y, area.w - area.w // 2, area.h)
        first, rest = (right, left) if h else (left, right)
    else:
        top = Rect(area.x, area.y, area.w, area.h // 2)
        bottom = Rect(area.x, area.y + area.h // 2, area.w, area.h - area.h // 2)
        first, rest = (bottom, top) if v else (top, bottom)
    return [first, *_bsp(i + 1, n - 1, rest, h, v)]


def tiles(layout: str, n: int, area: Rect, flip: str | None) -> list[Rect]:
    """The tiles of a layout, for `n` containers in `area`, as the layouts of the daemon mirror
    them: columns by a horizontal flip, rows by a vertical one, BSP by both."""
    h = flip in ("Horizontal", BOTH)
    v = flip in ("Vertical", BOTH)
    if n == 0:
        return []
    if layout == "Columns":
        cols = [Rect(area.x + i * area.w // n, area.y, area.w // n, area.h) for i in range(n)]
        return cols[::-1] if h and n >= 2 else cols
    if layout == "Rows":
        rows = [Rect(area.x, area.y + i * area.h // n, area.w, area.h // n) for i in range(n)]
        return rows[::-1] if v and n >= 2 else rows
    return _bsp(0, n, area, h, v)


class FakeDeskPointer:
    def __init__(self, desk: "FakeDesk") -> None:
        self.desk = desk

    def move(self, x: int, y: int) -> None:
        self.desk.cursor = (x, y)

    def down(self) -> None:
        pass

    def up(self) -> None:
        pass

    def drag(
        self, start: tuple[int, int], end: tuple[int, int], steps: int = 25, ms: int = 30
    ) -> None:
        pass


class FakeDesk:
    """The `bug`s: `send_follows` (a send takes the focus along), `move_stays` (a move does not),
    `send_ignores_workspace` (the workspace asked for is the one that is shown), `frame_stuck` (the
    window is on the other monitor but its frame is not), `focus_ignored` (`focus-monitor` does
    nothing), `cycle_stuck` (`cycle-monitor` does nothing), `workspace_stays` (the workspace move
    does nothing), `cursor_ignored` (`focus-monitor-at-cursor` does nothing), `ignores_option` (the
    config's `layout_flip` is not read), `option_on_all` (it is every workspace's), `swaps_axes`
    (it is read as the other axis), `command_sets` (`flip-layout` sets the flip, not toggles it)."""

    tolerance = 24

    def __init__(self, *, displays: int = 1, bug: str | None = None, pointer: bool = True) -> None:
        self.bug = bug
        self.has_pointer = pointer
        self.display_count = displays
        self.mons = [Mon([Space()]) for _ in range(displays)]
        self.focus_mon = 0
        self.active: str | None = None
        self.order: list[str] = []
        self.home: dict[str, int] = {}
        self.rules: list[tuple[str, int]] = []
        self.cursor = (0, 0)

    # -- the machine
    def has(self, feature: str) -> bool:
        return feature == "pointer" and self.has_pointer

    @property
    def pointer(self) -> FakeDeskPointer:
        return FakeDeskPointer(self)

    def wait_for(self, predicate: Any, timeout: float = 10) -> bool:
        return bool(predicate())

    def displays(self) -> list[Rect]:
        return DISPLAYS[: self.display_count]

    def work_area(self) -> Rect:
        return DISPLAYS[0]

    def add_display(self) -> None:
        self.display_count += 1
        self.mons.append(Mon([Space()]))

    def remove_display(self) -> None:
        gone = self.mons.pop()
        self.display_count -= 1
        self.mons[0].spaces[0].containers += [t for s in gone.spaces for t in s.containers]
        self.focus_mon = min(self.focus_mon, len(self.mons) - 1)
        self._refocus()

    def lock(self) -> None:
        raise NotImplementedError

    def unlock(self) -> None:
        raise NotImplementedError

    def move_window(self, client: Client, x: int, y: int) -> None:
        raise NotImplementedError

    def minimize(self, client: Client) -> None:
        raise NotImplementedError

    def rewrite_config(self, config: dict[str, Any]) -> None:
        raise NotImplementedError

    def screenshot(self, name: str) -> Image.Image:
        return Image.new("RGB", (4, 4))

    def resizable_display(self) -> int | None:
        return None

    def resize_display(self, index: int, width: int, height: int) -> None:
        pass

    def crash(self) -> None:
        pass

    def temp_path(self, name: str) -> str:
        return f"/tmp/{name}"

    def fullscreen(self, client: Client, on: bool = True) -> None:
        pass

    def user_maximize(self, client: Client, on: bool = True) -> None:
        pass

    def exe_id(self, program: str) -> str:
        return program

    # -- the daemon
    def start_komorebi(self) -> None:
        self._reset([Space()])

    def restart_with(self, config: dict[str, Any]) -> None:
        configured = config["monitors"][0]["workspaces"]
        flips = [w.get("layout_flip") for w in configured]
        if self.bug == "ignores_option":
            flips = [None] * len(configured)
        elif self.bug == "option_on_all":
            first = next((f for f in flips if f), None)
            flips = [first] * len(configured)
        elif self.bug == "swaps_axes":
            swap = {"Horizontal": "Vertical", "Vertical": "Horizontal"}
            flips = [swap.get(f, f) for f in flips]
        spaces = [
            Space(name=w.get("name"), layout=w["layout"], flip=flip)
            for w, flip in zip(configured, flips, strict=True)
        ]
        self._reset(spaces)
        self.rules = [
            (rule["id"], index)
            for index, w in enumerate(configured)
            for rule in w.get("workspace_rules", [])
        ]
        for title in list(self.order):
            target = self._by_rule(title)
            if target is not None:
                self._remove(title)
                self.mons[0].spaces[target].containers.append(title)
        self._refocus()

    def _reset(self, spaces: list[Space]) -> None:
        """A new daemon knows nothing: every window is tiled on the first workspace of the first
        monitor."""
        self.mons = [Mon(spaces)] + [Mon([Space()]) for _ in range(self.display_count - 1)]
        self.mons[0].spaces[0].containers = list(self.order)
        self.focus_mon = 0
        self.rules = []
        self._refocus()

    def _by_rule(self, title: str) -> int | None:
        for prefix, index in self.rules:
            if title.startswith(prefix):
                return index
        return None

    def _remove(self, title: str) -> None:
        for mon in self.mons:
            for space in mon.spaces:
                if title in space.containers:
                    space.containers.remove(title)
                    space.focus = max(0, min(space.focus, len(space.containers) - 1))

    def _find(self, title: str) -> tuple[int, int, int]:
        for m, mon in enumerate(self.mons):
            for w, space in enumerate(mon.spaces):
                if title in space.containers:
                    return m, w, space.containers.index(title)
        raise KeyError(title)

    def _shown(self, monitor: int) -> Space:
        mon = self.mons[monitor]
        return mon.spaces[mon.shown]

    def _refocus(self) -> None:
        """The window of the focused container of the monitor that has the focus has it, if
        there is one: the focus is not moved when it has no window to go to."""
        space = self._shown(self.focus_mon)
        if space.containers:
            self.active = space.containers[min(space.focus, len(space.containers) - 1)]

    def _show(self, monitor: int, workspace: int) -> None:
        mon = self.mons[monitor]
        while len(mon.spaces) <= workspace:
            mon.spaces.append(Space())
        mon.shown = workspace

    def spawn(
        self,
        title: str,
        app_id: str | None = None,
        exe: str | None = None,
        rename_to: str | None = None,
        after: float = 0,
    ) -> Client:
        self.order.append(title)
        target = self._by_rule(title)
        if target is not None:
            self.mons[0].spaces[target].containers.append(title)
            self.home[title] = 0
        else:
            space = self._shown(self.focus_mon)
            space.containers.append(title)
            space.focus = len(space.containers) - 1
            self.home[title] = self.focus_mon
            self.active = title
        return Client(title)

    def close(self, client: Client) -> None:
        self._remove(client.title)
        self.order.remove(client.title)
        if self.active == client.title:
            self.active = None
        self._refocus()

    def activate(self, client: Client) -> None:
        m, w, i = self._find(client.title)
        self.focus_mon = m
        self.mons[m].shown = w
        self.mons[m].spaces[w].focus = i
        self.active = client.title

    def hidden(self, client: Client) -> bool:
        m, w, _ = self._find(client.title)
        return self.mons[m].shown != w

    def os_window(self, client: Client) -> OsWindow | None:
        try:
            m, w, i = self._find(client.title)
        except KeyError:
            return None
        space = self.mons[m].spaces[w]
        rect = tiles(space.layout, len(space.containers), DISPLAYS[m], space.flip)[i]
        if self.bug == "frame_stuck":
            dx = DISPLAYS[self.home[client.title]].x - DISPLAYS[m].x
            rect = Rect(rect.x + dx, rect.y, rect.w, rect.h)
        return OsWindow(client.title, rect, focused=self.active == client.title)

    # -- the commands
    def komorebic_status(self, *args: str) -> tuple[int, str]:
        try:
            return 0, self.komorebic(*args)
        except AssertionError:
            return 2, f"error: unrecognized subcommand '{args[0]}'"

    def _move(self, target: int, workspace: int | None, follow: bool) -> None:
        """`move_container_to_monitor`: the focused container leaves, and the monitor it goes to
        shows the workspace it is put on."""
        if target >= len(self.mons):
            return
        if self.bug == "send_ignores_workspace":
            workspace = None
        source = self._shown(self.focus_mon)
        if not source.containers:
            return
        title = source.containers.pop(min(source.focus, len(source.containers) - 1))
        source.focus = max(0, min(source.focus, len(source.containers) - 1))
        if workspace is not None:
            self._show(target, workspace)
        space = self._shown(target)
        space.containers.append(title)
        space.focus = len(space.containers) - 1
        if follow:
            self.focus_mon = target
        self._refocus()

    def _move_workspace(self, target: int) -> None:
        if target >= len(self.mons) or self.bug == "workspace_stays":
            return
        mon = self.mons[self.focus_mon]
        space = mon.spaces.pop(mon.shown)
        mon.shown = max(0, mon.shown - 1)
        if not mon.spaces:
            mon.spaces.append(Space())
        there = self.mons[target]
        there.spaces.append(space)
        there.shown = len(there.spaces) - 1
        self.focus_mon = target
        self._refocus()

    def _by_name(self, name: str) -> tuple[int, int] | None:
        for m, mon in enumerate(self.mons):
            for w, space in enumerate(mon.spaces):
                if space.name == name:
                    return m, w
        return None

    def _cycle(self, way: str) -> int:
        step = 1 if way == "next" else -1
        return (self.focus_mon + step) % len(self.mons)

    def komorebic(self, *args: str) -> str:
        sends_follow = self.bug == "send_follows"
        moves_follow = self.bug != "move_stays"
        match args:
            case ("focus-monitor", number):
                if int(number) < len(self.mons) and self.bug != "focus_ignored":
                    self.focus_mon = int(number)
                    self._refocus()
            case ("cycle-monitor", way):
                if self.bug != "cycle_stuck":
                    self.focus_mon = self._cycle(way)
                    self._refocus()
            case ("focus-monitor-at-cursor",):
                for m, display in enumerate(self.displays()):
                    x, y = self.cursor
                    inside = display.x <= x < display.x + display.w
                    inside = inside and display.y <= y < display.y + display.h
                    if inside and self.bug != "cursor_ignored":
                        self.focus_mon = m
            case ("focus-monitor-workspace", monitor, workspace):
                if (self.focus_mon, self.mons[self.focus_mon].shown) != (
                    int(monitor),
                    int(workspace),
                ):
                    self.focus_mon = int(monitor)
                    self._show(self.focus_mon, int(workspace))
                    self._refocus()
            case ("focus-workspace", workspace):
                self._show(self.focus_mon, int(workspace))
                self._refocus()
            case ("send-to-monitor", number):
                self._move(int(number), None, sends_follow)
            case ("move-to-monitor", number):
                self._move(int(number), None, moves_follow)
            case ("cycle-send-to-monitor", way):
                self._move(self._cycle(way), None, sends_follow)
            case ("cycle-move-to-monitor", way):
                self._move(self._cycle(way), None, moves_follow)
            case ("send-to-monitor-workspace", monitor, workspace):
                self._move(int(monitor), int(workspace), sends_follow)
            case ("move-to-monitor-workspace", monitor, workspace):
                self._move(int(monitor), int(workspace), moves_follow)
            case ("send-to-named-workspace", name):
                if (found := self._by_name(name)) is not None:
                    self._move(found[0], found[1], sends_follow)
            case ("move-to-named-workspace", name):
                if (found := self._by_name(name)) is not None:
                    self._move(found[0], found[1], moves_follow)
            case ("cycle-move-workspace-to-monitor", way):
                self._move_workspace(self._cycle(way))
            case ("move-workspace-to-monitor", number):
                self._move_workspace(int(number))
            case ("ensure-named-workspaces", monitor, *names):
                for i, name in enumerate(names):
                    self._show_free(int(monitor), i).name = name
            case ("ensure-workspaces", monitor, count):
                self._show_free(int(monitor), int(count) - 1)
            case ("flip-layout", axis):
                space = self._shown(self.focus_mon)
                if self.bug == "command_sets":
                    space.flip = BOTH if AXES[axis] == "HV" else AXES[axis]
                else:
                    space.flip = _toggle(space.flip, AXES[axis])
            case _:
                raise AssertionError(f"the fake does not know komorebic {args}")
        return ""

    def _show_free(self, monitor: int, workspace: int) -> Space:
        """The workspace of a monitor, made if it is not there. It is not shown."""
        mon = self.mons[monitor]
        while len(mon.spaces) <= workspace:
            mon.spaces.append(Space())
        return mon.spaces[workspace]

    def state(self) -> Snapshot:
        def rect(r: Rect) -> dict[str, int]:
            return {"left": r.x, "top": r.y, "right": r.w, "bottom": r.h}

        def window(title: str, r: Rect) -> dict[str, Any]:
            return {
                "id": hash(title) % 100000,
                "rect": rect(r),
                "details": {"title": title, "exe": "foot", "class": "", "path": ""},
            }

        def workspace(space: Space, area: Rect) -> dict[str, Any]:
            laid = tiles(space.layout, len(space.containers), area, space.flip)
            return {
                "name": space.name,
                "containers": {
                    "elements": [
                        {
                            "id": str(i),
                            "locked": False,
                            "windows": {"elements": [window(t, laid[i])], "focused": 0},
                        }
                        for i, t in enumerate(space.containers)
                    ],
                    "focused": space.focus,
                },
                "monocle_container": None,
                "maximized_window": None,
                "floating_windows": {"elements": [], "focused": 0},
                "layout": {"Default": space.layout},
                "tile": True,
                "latest_layout": [rect(r) for r in laid],
            }

        monitors = [
            {
                "id": m + 1,
                "size": rect(DISPLAYS[m]),
                "workspaces": {
                    "elements": [workspace(s, DISPLAYS[m]) for s in mon.spaces],
                    "focused": mon.shown,
                },
            }
            for m, mon in enumerate(self.mons)
        ]
        return parse({"monitors": {"elements": monitors, "focused": self.focus_mon}})
