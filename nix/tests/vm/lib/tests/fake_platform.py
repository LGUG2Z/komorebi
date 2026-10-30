"""A desktop in memory with a daemon that follows the commands of the scenarios.

Three tiles in a row, a window in each. `focus`, `cycle-focus`, `move` and `cycle-move` do what the
real daemon does with windows laid out left to right, so that a scenario can be run against a daemon
that is right, and against ones that are wrong in one way.
"""

from typing import Any

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow
from komorebi_vm.core.scenarios.commands import PRINTED
from komorebi_vm.core.snapshot import Snapshot, parse

TILES = [Rect(0, 0, 640, 1080), Rect(640, 0, 640, 1080), Rect(1280, 0, 640, 1080)]


def _place_index(place: str, count: int) -> int:
    """The index a placement rule's place stands for: `Primary`, `Last`, or a number from 1."""
    return int(place) - 1 if place.isdigit() else {"Primary": 0, "Last": count}[place]


class FakePointer:
    """A pointer that, dragged from just outside the right edge of a tile, moves that edge."""

    def __init__(self, platform: "FakePlatform") -> None:
        self.platform = platform
        self.at = (0, 0)

    def move(self, x: int, y: int) -> None:
        self.at = (x, y)
        if self.platform.follows_mouse:
            for i, tile in enumerate(self.platform.tiles[: len(self.platform.containers)]):
                if tile.x <= x < tile.x + tile.w and tile.y <= y < tile.y + tile.h:
                    self.platform.focus = i

    def down(self) -> None:
        pass

    def up(self) -> None:
        pass

    def drag(
        self, start: tuple[int, int], end: tuple[int, int], steps: int = 25, ms: int = 30
    ) -> None:
        self.platform.resize(start, end[0] - start[0])


class FakePlatform:
    tolerance = 24

    def __init__(
        self,
        *,
        moves: bool = True,
        focus_follows_move: bool = True,
        pointer: bool = True,
        follows_pointer: bool = True,
        reloads: bool = True,
        retiles_on_unlock: bool = True,
    ) -> None:
        self.tiles = list(TILES)
        self.has_pointer = pointer
        self.follows_pointer = follows_pointer
        self.spaces: list[list[list[str]]] = [[], [], []]
        """The two workspaces: for each, the stack of each container, in the order of the tiles,
        and the first window of a stack is the one in view."""
        self.focuses = [0, 0, 0]
        self.current = 0
        self.monocle: str | None = None
        self.initial_rules: dict[str, int] = {}
        self.rules: dict[str, int] = {}
        self.ignored: set[str] = set()
        self.placing = False
        self.follows_mouse = False
        self.mouse_ignored = False
        self.prefix_rules: list[tuple[str, int]] = []
        self.unmanaged: list[str] = []
        self.display_count = 2
        self.display_of: dict[str, int] = {}
        self.forgets_displays = False
        self.floats: list[str] = []
        self.class_rules: dict[str, int] = {}
        self.ignored_classes: set[str] = set()
        self.exe_rules: dict[str, int] = {}
        self.ignored_exes: set[str] = set()
        self.exes: dict[str, str] = {}
        self.names: list[str | None] = [None, None, None]
        self.layout_name = "Columns"
        self.reloads = reloads
        self.locked = False
        self.moved: dict[str, Rect] = {}
        self.retiles_on_unlock = retiles_on_unlock
        self.tiling = True
        self.paused = False
        self.flipped = False
        self.last = 0
        self.maximized: str | None = None
        self.workspace_padding = 0
        self.container_padding = 0
        self.minimized: list[str] = []
        self.running = True
        self.placement: Any = None
        self.moves = moves
        self.focus_follows_move = focus_follows_move

    def has(self, feature: str) -> bool:
        return feature == "lock" or (feature == "pointer" and self.has_pointer)

    def lock(self) -> None:
        self.locked = True

    def unlock(self) -> None:
        self.locked = False
        if self.retiles_on_unlock:
            self.moved.clear()

    def move_window(self, client: Client, x: int, y: int) -> None:
        here = self.os_window(client)
        assert here is not None
        self.moved[client.title] = Rect(x, y, here.frame.w, here.frame.h)

    @property
    def pointer(self) -> FakePointer:
        return FakePointer(self)

    def resize(self, start: tuple[int, int], dx: int) -> None:
        """The edge of the tile that `start` is 4 pixels outside of moves by `dx`, when the daemon
        follows the pointer."""
        if not self.follows_pointer:
            return
        for i, tile in enumerate(self.tiles[:-1]):
            if abs(start[0] - (tile.x + tile.w + 4)) <= 1:
                a, b = tile, self.tiles[i + 1]
                self.tiles[i] = Rect(a.x, a.y, a.w + dx, a.h)
                self.tiles[i + 1] = Rect(b.x + dx, b.y, b.w - dx, b.h)
                return

    def wait_for(self, predicate: Any, timeout: float = 10) -> bool:
        return bool(predicate())

    def start_komorebi(self) -> None:
        """A new daemon knows nothing: every window is tiled on the first workspace."""
        everyone = [t for containers in self.spaces for stack in containers for t in stack]
        everyone += self.floats
        self.floats = []
        self.spaces = [[[t] for t in everyone], [], []]
        self.focuses = [0, 0, 0]
        self.current = 0
        self.monocle = None
        self.running = True
        self._relayout()

    @property
    def containers(self) -> list[list[str]]:
        return self.spaces[self.current]

    @property
    def focus(self) -> int:
        return self.focuses[self.current]

    @focus.setter
    def focus(self, value: int) -> None:
        self.focuses[self.current] = value

    @property
    def order(self) -> list[str]:
        """The titles in view, in the order of the tiles."""
        return [stack[0] for stack in self.containers]

    def spawn(self, title: str, app_id: str | None = None, exe: str | None = None) -> Client:
        self.exes[title] = exe or ""
        if exe in self.ignored_exes:
            self.unmanaged.append(title)
            return Client(title)
        if exe in self.exe_rules:
            self.spaces[self.exe_rules[exe]].append([title])
            return Client(title)
        if app_id in self.ignored_classes:
            self.unmanaged.append(title)
            return Client(title)
        if app_id in self.class_rules:
            self.spaces[self.class_rules[app_id]].append([title])
            return Client(title)
        if self.paused:
            self.unmanaged.append(title)
            return Client(title)
        if title in self.ignored:
            self.unmanaged.append(title)
        elif title in self.initial_rules:
            self.spaces[self.initial_rules.pop(title)].append([title])
        elif title in self.rules:
            self.spaces[self.rules[title]].append([title])
        elif (target := self._by_prefix(title)) is not None:
            self.spaces[target].append([title])
        elif self.placing:
            self._place(title)
        else:
            self.containers.append([title])
        if len(self.containers) > len(self.tiles):
            self._relayout()
        return Client(title)

    def _place(self, title: str) -> None:
        """A window goes where the placement rules say, or after the one in focus, and has the
        focus."""
        n = len(self.containers)
        index = min(self.focus + 1, n) if n else 0
        rules = self.placement
        if isinstance(rules, dict):
            for place, matches in rules.items():
                for match in matches if isinstance(matches, list) else [matches]:
                    equals = match["matching_strategy"] == "Equals"
                    if (title == match["id"]) if equals else (match["id"] in title):
                        index = _place_index(place, n)
        elif rules is not None:
            index = 0 if rules == "Primary" else int(rules) - 1
        index = max(0, min(index, n))
        self.containers.insert(index, [title])
        self.focus = index

    def _by_prefix(self, title: str) -> int | None:
        for prefix, space in self.prefix_rules:
            if title.startswith(prefix):
                return space
        return None

    def rewrite_config(self, config: dict[str, Any]) -> None:
        if self.reloads:
            self._load(config)

    def _load(self, config: dict[str, Any]) -> None:
        workspaces = config["monitors"][0]["workspaces"]
        self.names = [w.get("name") for w in workspaces] + [None] * (3 - len(workspaces))
        self.layout_name = workspaces[0]["layout"].title()
        self.prefix_rules = [
            (rule["id"], index)
            for index, workspace in enumerate(workspaces)
            for rule in workspace.get("workspace_rules", [])
            if rule["matching_strategy"] == "StartsWith"
        ]

    def restart_with(self, config: dict[str, Any]) -> None:
        self.start_komorebi()
        self._load(config)
        workspaces = config["monitors"][0]["workspaces"]
        self.placement = workspaces[0].get("initial_window_placement_rules")
        for title in [t for stack in self.spaces[0] for t in stack]:
            target = self._by_prefix(title)
            if target not in (None, 0):
                self._find_remove(title)
                self.spaces[target].append([title])
        self._relayout()
        self.placing = True

    def _find_remove(self, title: str) -> None:
        space, index = self._find(title)
        self.spaces[space].pop(index)

    def _find(self, title: str) -> tuple[int, int]:
        for s, containers in enumerate(self.spaces):
            for i, stack in enumerate(containers):
                if title in stack:
                    return s, i
        raise KeyError(title)

    def displays(self) -> list[Rect]:
        return [Rect(i * 1920, 0, 1920, 1080) for i in range(self.display_count)]

    def add_display(self) -> None:
        self.display_count += 1

    def remove_display(self) -> None:
        self.display_count -= 1
        for title, display in self.display_of.items():
            if display >= self.display_count:
                self.display_of[title] = 0

    def close(self, client: Client) -> None:
        if client.title in self.floats:
            self.floats.remove(client.title)
            return
        if client.title in self.minimized:
            self.minimized.remove(client.title)
            return
        if client.title in self.unmanaged:
            self.unmanaged.remove(client.title)
            return
        s, i = self._find(client.title)
        stack = self.spaces[s][i]
        stack.remove(client.title)
        if not stack:
            self.spaces[s].pop(i)
            self.focuses[s] = max(0, min(self.focuses[s], len(self.spaces[s]) - 1))
        if self.monocle == client.title:
            self.monocle = None
        self._relayout()

    def _relayout(self) -> None:
        """The tiles of each workspace share the work area in equal columns."""
        self.tiles = list(TILES)
        n = len(self.containers)
        if n:
            w, c = self.workspace_padding, self.container_padding
            width = (1920 - 2 * w) // n
            self.tiles = [
                Rect(w + i * width + c, w + c, width - 2 * c, 1080 - 2 * w - 2 * c)
                for i in range(n)
            ] + list(TILES[n:])
            if self.flipped:
                self.tiles = [Rect(1920 - t.x - t.w, t.y, t.w, t.h) for t in self.tiles[:n]] + list(
                    TILES[n:]
                )

    def _container(self, title: str) -> int:
        return self._find(title)[1]

    def hidden(self, client: Client) -> bool:
        if client.title in self.floats or client.title in self.minimized:
            return False
        if client.title in self.unmanaged:
            return False
        s, i = self._find(client.title)
        away = self.monocle is not None and client.title != self.monocle
        return s != self.current or self.spaces[s][i][0] != client.title or away

    def work_area(self) -> Rect:
        return Rect(0, 0, 1920, 1080)

    def minimize(self, client: Client) -> None:
        self._find_remove(client.title)
        self.minimized.append(client.title)
        self.focuses[self.current] = max(0, min(self.focus, len(self.containers) - 1))
        self._relayout()

    def activate(self, client: Client) -> None:
        if client.title in self.minimized:
            self.minimized.remove(client.title)
            self.containers.append([client.title])
            self._relayout()
        self.current, self.focus = self._find(client.title)
        stack = self.containers[self.focus]
        stack.insert(0, stack.pop(stack.index(client.title)))

    def screenshot(self, name: str) -> Image.Image:
        return Image.new("RGB", (4, 4))

    def os_window(self, client: Client) -> OsWindow | None:
        if client.title == self.maximized:
            return OsWindow(client.title, Rect(0, 0, 1920, 1080), focused=True)
        if client.title in self.minimized:
            return OsWindow(client.title, Rect(0, 0, 0, 0), focused=False)
        if client.title in self.floats:
            return OsWindow(client.title, Rect(300, 300, 600, 400), focused=False)
        if client.title in self.unmanaged:
            return OsWindow(client.title, Rect(100, 100, 400, 300), focused=False)
        try:
            s, i = self._find(client.title)
        except KeyError:
            return None
        if self.monocle == client.title:
            return OsWindow(client.title, Rect(0, 0, 1920, 1080), focused=True)
        focused = (
            s == self.current and i == self.focuses[s] and self.spaces[s][i][0] == client.title
        )
        tile = self.tiles[i]
        shift = 1920 * self.display_of.get(client.title, 0)
        if client.title in self.moved:
            return OsWindow(client.title, self.moved[client.title], focused=focused)
        return OsWindow(client.title, Rect(tile.x + shift, tile.y, tile.w, tile.h), focused=focused)

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        """The fake's answer as a real `komorebic` gives it: exit 0, or 2 for what it lacks."""
        if not self.running:
            return 1, "the daemon is not running"
        if args[:1] == ("state",) or " ".join(args) in PRINTED:
            return 0, "{}"
        try:
            return 0, self.komorebic(*args)
        except AssertionError:
            return 2, f"error: unrecognized subcommand '{args[0]}'"

    def komorebic(self, *args: str) -> str:
        n = len(self.containers)
        match args:
            case ("stop",):
                self.running = False
            case ("focus-follows-mouse", "enable" | "disable" as state):
                self.follows_mouse = state == "enable" and not self.mouse_ignored
            case ("toggle-focus-follows-mouse", *_):
                self.follows_mouse = not self.follows_mouse and not self.mouse_ignored
            case ("focus", "right"):
                self.focus = min(self.focus + 1, n - 1)
            case ("focus", "left"):
                self.focus = max(self.focus - 1, 0)
            case ("cycle-focus", "next"):
                self.focus = (self.focus + 1) % n
            case ("cycle-focus", "previous"):
                self.focus = (self.focus - 1) % n
            case ("move", "right"):
                self._swap(min(self.focus + 1, n - 1))
            case ("cycle-move", "previous"):
                self._swap((self.focus - 1) % n)
            case ("stack", "left"):
                if self.focus > 0:
                    stack = self.containers.pop(self.focus)
                    self.focus -= 1
                    self.containers[self.focus][:0] = stack
            case ("cycle-stack", "next" | "previous" as way):
                stack = self.containers[self.focus]
                stack.append(stack.pop(0)) if way == "next" else stack.insert(0, stack.pop())
            case ("send-to-workspace", number):
                stack = self.containers[self.focus]
                window = stack.pop(0)
                if not stack:
                    self.containers.pop(self.focus)
                    self.focus = max(0, min(self.focus, len(self.containers) - 1))
                self.spaces[int(number)].append([window])
            case ("change-layout", _):
                pass
            case ("send-to-monitor", _):
                pass
            case ("move-to-monitor", number):
                if int(number) < self.display_count:
                    self.display_of[self.containers[self.focus][0]] = int(number)
            case ("toggle-monocle",):
                top = self.containers[self.focus][0]
                self.monocle = None if self.monocle else top
            case ("ensure-workspaces", _, _) | ("clear-all-workspace-rules",):
                if args[0] == "clear-all-workspace-rules":
                    self.class_rules.clear()
                    self.exe_rules.clear()
                    self.rules.clear()
                    self.initial_rules.clear()
            case ("initial-workspace-rule", "title", title, _, number):
                self.initial_rules[title] = int(number)
            case ("workspace-rule", "title", title, _, number):
                self.rules[title] = int(number)
            case ("workspace-rule", "class", name, _, number):
                self.class_rules[name] = int(number)
            case ("workspace-rule", "exe", name, _, number):
                self.exe_rules[name] = int(number)
            case ("ignore-rule", "exe", name):
                self.ignored_exes.add(name)
            case ("ignore-rule", "class", name):
                self.ignored_classes.add(name)
            case ("ignore-rule", "title", title):
                self.ignored.add(title)
            case ("move-to-workspace", number):
                self.komorebic("send-to-workspace", number)
                self.last, self.current = self.current, int(number)
                self.focus = len(self.containers) - 1
            case ("enforce-workspace-rules",):
                for title, number in self.rules.items():
                    for s, containers in enumerate(self.spaces):
                        for stack in containers:
                            if title in stack and s != number:
                                stack.remove(title)
                                if not stack:
                                    containers.remove(stack)
                                self.spaces[number].append([title])
                                break
            case ("focus-workspace", number):
                self.last, self.current = self.current, int(number)
            case ("focus-last-workspace",):
                self.last, self.current = self.current, self.last
            case ("focused-workspace-padding", size):
                self.workspace_padding = int(size)
                self._relayout()
            case ("focused-workspace-container-padding", size):
                self.container_padding = int(size)
                self._relayout()
            case ("toggle-maximize",):
                self.maximized = None if self.maximized else self.containers[self.focus][0]
            case ("resize-axis", "horizontal", way):
                d = 50 if way == "increase" else -50
                i = self.focus
                if n > 1 and i < n - 1:
                    a, b = self.tiles[i], self.tiles[i + 1]
                    self.tiles[i] = Rect(a.x, a.y, a.w + d, a.h)
                    self.tiles[i + 1] = Rect(b.x + d, b.y, b.w - d, b.h)
            case ("promote",):
                self.containers.insert(0, self.containers.pop(self.focus))
                self.focus = 0
            case ("flip-layout", "horizontal"):
                self.flipped = not self.flipped
                self._relayout()
            case ("toggle-pause",):
                self.paused = not self.paused
            case ("toggle-tiling",):
                self.tiling = not self.tiling
            case ("toggle-float",):
                if self.floats:
                    self.containers.insert(self.focus, [self.floats.pop()])
                else:
                    title = self.containers[self.focus][0]
                    self._find_remove(title)
                    self.floats.append(title)
                    self.focuses[self.current] = max(0, min(self.focus, len(self.containers) - 1))
                self._relayout()
            case ("unstack",):
                stack = self.containers.pop(self.focus)
                self.containers[self.focus : self.focus] = [[t] for t in stack]
            case _:
                raise AssertionError(f"the fake does not know komorebic {args}")
        return ""

    def _swap(self, other: int) -> None:
        if not self.moves:
            return
        self.containers[self.focus], self.containers[other] = (
            self.containers[other],
            self.containers[self.focus],
        )
        if self.focus_follows_move:
            self.focus = other

    def state(self) -> Snapshot:
        def window(title: str, tile: Rect) -> dict[str, Any]:
            return {
                "id": hash(title) % 1000,
                "rect": {"left": tile.x, "top": tile.y, "right": tile.w, "bottom": tile.h},
                "details": {
                    "title": title,
                    "exe": self.exes.get(title, ""),
                    "class": "",
                    "path": "",
                },
            }

        def rect(r: Rect) -> dict[str, int]:
            return {"left": r.x, "top": r.y, "right": r.w, "bottom": r.h}

        def workspace(
            index: int, containers: list[list[str]], focus: int, current: bool
        ) -> dict[str, Any]:
            floats = self.floats if current else []
            monocle = None
            if current and self.monocle is not None:
                whole = Rect(0, 0, 1920, 1080)
                windows = {"elements": [window(self.monocle, whole)], "focused": 0}
                monocle = {"id": "m", "locked": False, "windows": windows}
            return {
                "name": self.names[index],
                "containers": {
                    "elements": [
                        {
                            "id": str(i),
                            "locked": False,
                            "windows": {
                                "elements": [window(t, self.tiles[i]) for t in stack],
                                "focused": 0,
                            },
                        }
                        for i, stack in enumerate(containers)
                    ],
                    "focused": focus,
                },
                "monocle_container": monocle,
                "maximized_window": (
                    window(self.maximized, Rect(0, 0, 1920, 1080))
                    if current and self.maximized
                    else None
                ),
                "floating_windows": {
                    "elements": [window(t, Rect(300, 300, 600, 400)) for t in floats],
                    "focused": 0,
                },
                "layout": {"Default": self.layout_name},
                "tile": self.tiling,
                "workspace_padding": self.workspace_padding,
                "container_padding": self.container_padding,
                "latest_layout": [rect(r) for r in self.tiles[: len(containers)]],
            }

        monitor = {
            "id": 1,
            "size": rect(Rect(0, 0, 1920, 1080)),
            "workspaces": {
                "elements": [
                    workspace(i, c, f, i == self.current)
                    for i, (c, f) in enumerate(zip(self.spaces, self.focuses, strict=True))
                ],
                "focused": self.current,
            },
        }
        extra = [
            {
                "id": n + 1,
                "size": rect(Rect(n * 1920, 0, 1920, 1080)),
                "workspaces": {"elements": [workspace(0, [], 0, False)], "focused": 0},
            }
            for n in range(1, 1 if self.forgets_displays else self.display_count)
        ]
        return parse({"monitors": {"elements": [monitor, *extra], "focused": 0}})
