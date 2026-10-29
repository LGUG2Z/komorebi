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


class FakePlatform:
    tolerance = 24

    def __init__(self, *, moves: bool = True, focus_follows_move: bool = True) -> None:
        self.order: list[str] = []
        """The titles in the order of the tiles."""
        self.focus = 0
        self.moves = moves
        self.focus_follows_move = focus_follows_move

    def wait_for(self, predicate: Any, timeout: float = 10) -> bool:
        return bool(predicate())

    def start_komorebi(self) -> None:
        pass

    def spawn(self, title: str) -> Client:
        self.order.append(title)
        return Client(title)

    def work_area(self) -> Rect:
        return Rect(0, 0, 1920, 1080)

    def activate(self, client: Client) -> None:
        self.focus = self.order.index(client.title)

    def screenshot(self, name: str) -> Image.Image:
        return Image.new("RGB", (4, 4))

    def os_window(self, client: Client) -> OsWindow | None:
        if client.title not in self.order:
            return None
        i = self.order.index(client.title)
        return OsWindow(client.title, TILES[i], focused=i == self.focus)

    def komorebic(self, *args: str) -> str:
        n = len(self.order)
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
            case _:
                raise AssertionError(f"the fake does not know komorebic {args}")
        return ""

    def _swap(self, other: int) -> None:
        if not self.moves:
            return
        self.order[self.focus], self.order[other] = self.order[other], self.order[self.focus]
        if self.focus_follows_move:
            self.focus = other

    def state(self) -> Snapshot:
        windows = [
            {
                "id": i + 1,
                "rect": {"left": r.x, "top": r.y, "right": r.w, "bottom": r.h},
                "details": {"title": t, "exe": "", "class": "", "path": ""},
            }
            for i, (t, r) in enumerate(zip(self.order, TILES, strict=False))
        ]
        workspace = {
            "name": None,
            "containers": {
                "elements": [
                    {"id": str(i), "locked": False, "windows": {"elements": [w], "focused": 0}}
                    for i, w in enumerate(windows)
                ],
                "focused": self.focus,
            },
            "monocle_container": None,
            "maximized_window": None,
            "floating_windows": {"elements": [], "focused": 0},
            "layout": {"Default": "Columns"},
            "tile": True,
            "latest_layout": [
                {"left": r.x, "top": r.y, "right": r.w, "bottom": r.h} for r in TILES
            ],
        }
        size = {"left": 0, "top": 0, "right": 1920, "bottom": 1080}
        monitor = {"id": 1, "size": size, "workspaces": {"elements": [workspace], "focused": 0}}
        return parse({"monitors": {"elements": [monitor], "focused": 0}})
