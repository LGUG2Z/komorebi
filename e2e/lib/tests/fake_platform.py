"""A desktop in memory with a daemon that follows the commands of the scenarios.

Three tiles in a row, a window in each. `focus`, `cycle-focus`, `move` and `cycle-move` do what the
real daemon does with windows laid out left to right, so that a scenario can be run against a daemon
that is right, and against ones that are wrong in one way.
"""

from typing import Any

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow
from komorebi_vm.core.snapshot import Snapshot, parse

TILES = [Rect(0, 0, 640, 1080), Rect(640, 0, 640, 1080), Rect(1280, 0, 640, 1080)]


class FakePointer:
    """A pointer that, dragged from just outside the right edge of a tile, moves that edge."""

    def __init__(self, platform: "FakePlatform") -> None:
        self.platform = platform
        self.at = (0, 0)

    def move(self, x: int, y: int) -> None:
        self.at = (x, y)

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
    ) -> None:
        self.tiles = list(TILES)
        self.has_pointer = pointer
        self.follows_pointer = follows_pointer
        self.spaces: list[list[list[str]]] = [[], []]
        """The two workspaces: for each, the stack of each container, in the order of the tiles,
        and the first window of a stack is the one in view."""
        self.focuses = [0, 0]
        self.current = 0
        self.moves = moves
        self.focus_follows_move = focus_follows_move

    def has(self, feature: str) -> bool:
        return feature == "pointer" and self.has_pointer

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
        pass

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

    def spawn(self, title: str) -> Client:
        self.containers.append([title])
        return Client(title)

    def _find(self, title: str) -> tuple[int, int]:
        for s, containers in enumerate(self.spaces):
            for i, stack in enumerate(containers):
                if title in stack:
                    return s, i
        raise AssertionError(f"no window {title}")

    def _container(self, title: str) -> int:
        return self._find(title)[1]

    def hidden(self, client: Client) -> bool:
        s, i = self._find(client.title)
        return s != self.current or self.spaces[s][i][0] != client.title

    def work_area(self) -> Rect:
        return Rect(0, 0, 1920, 1080)

    def activate(self, client: Client) -> None:
        self.current, self.focus = self._find(client.title)
        stack = self.containers[self.focus]
        stack.insert(0, stack.pop(stack.index(client.title)))

    def screenshot(self, name: str) -> Image.Image:
        return Image.new("RGB", (4, 4))

    def os_window(self, client: Client) -> OsWindow | None:
        i = self._container(client.title)
        return OsWindow(
            client.title,
            TILES[i],
            focused=i == self.focus and self.containers[i][0] == client.title,
        )

    def komorebic(self, *args: str) -> str:
        n = len(self.containers)
        match args:
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
            case ("focus-workspace", number):
                self.current = int(number)
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
                "details": {"title": title, "exe": "", "class": "", "path": ""},
            }

        def rect(r: Rect) -> dict[str, int]:
            return {"left": r.x, "top": r.y, "right": r.w, "bottom": r.h}

        def workspace(containers: list[list[str]], focus: int) -> dict[str, Any]:
            return {
                "name": None,
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
                "monocle_container": None,
                "maximized_window": None,
                "floating_windows": {"elements": [], "focused": 0},
                "layout": {"Default": "Columns"},
                "tile": True,
                "latest_layout": [rect(r) for r in self.tiles[: len(containers)]],
            }

        monitor = {
            "id": 1,
            "size": rect(Rect(0, 0, 1920, 1080)),
            "workspaces": {
                "elements": [
                    workspace(c, f) for c, f in zip(self.spaces, self.focuses, strict=True)
                ],
                "focused": self.current,
            },
        }
        return parse({"monitors": {"elements": [monitor], "focused": 0}})
