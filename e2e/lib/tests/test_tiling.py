"""The tiling scenario against a platform that tiles correctly, and against ones that do not."""

from PIL import Image

from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow, Pointer
from komorebi_vm.core.scenarios import tiling
from komorebi_vm.core.snapshot import Snapshot, parse


class FakePlatform:
    """A desktop in memory: `spawn` opens a window, and the frames are the ones it is given."""

    tolerance = 24

    def __init__(self, frames: list[Rect]) -> None:
        self.frames = frames
        self.opened: list[str] = []

    def wait_for(self, predicate, timeout: float = 10) -> bool:  # noqa: ANN001
        return predicate()

    def spawn(self, title: str) -> Client:
        self.opened.append(title)
        return Client(title)

    def state(self) -> Snapshot:
        return parse(_state(self.opened))

    def os_window(self, client: Client) -> OsWindow | None:
        i = self.opened.index(client.title)
        return OsWindow(client.title, self.frames[i], focused=False)

    def hidden(self, client: Client) -> bool:
        return False

    def close(self, client: Client) -> None:
        pass

    def has(self, feature: str) -> bool:
        return False

    @property
    def pointer(self) -> Pointer:
        raise AssertionError("no pointer")

    def work_area(self) -> Rect:
        return Rect(0, 0, 1920, 1080)

    def start_komorebi(self) -> None:
        pass

    def komorebic(self, *args: str) -> str:
        return ""

    def activate(self, client: Client) -> None:
        pass

    def screenshot(self, name: str) -> Image.Image:
        return Image.new("RGB", (4, 4))


def _state(titles: list[str]) -> dict:
    width = 1920 // len(titles)
    rects = [
        {"left": i * width, "top": 0, "right": width, "bottom": 1080} for i in range(len(titles))
    ]
    windows = [
        {"id": i + 1, "rect": rects[i], "details": {"title": t, "exe": "", "class": "", "path": ""}}
        for i, t in enumerate(titles)
    ]
    workspace = {
        "name": None,
        "containers": {
            "elements": [
                {"id": str(i), "locked": False, "windows": {"elements": [w], "focused": 0}}
                for i, w in enumerate(windows)
            ],
            "focused": 0,
        },
        "monocle_container": None,
        "maximized_window": None,
        "floating_windows": {"elements": [], "focused": 0},
        "layout": {"Default": "Columns"},
        "tile": True,
        "latest_layout": rects,
    }
    monitor = {
        "id": 1,
        "size": {"left": 0, "top": 0, "right": 1920, "bottom": 1080},
        "workspaces": {"elements": [workspace], "focused": 0},
    }
    return {"monitors": {"elements": [monitor], "focused": 0}}


def test_windows_that_are_tiled_pass() -> None:
    platform = FakePlatform(
        [Rect(0, 0, 640, 1080), Rect(640, 0, 640, 1080), Rect(1280, 0, 640, 1080)]
    )
    checks = Checks()
    tiling.run(platform, checks)
    assert checks.failed == 0
    assert checks.passed > 0


def test_windows_over_each_other_fail() -> None:
    platform = FakePlatform(
        [Rect(0, 0, 700, 1080), Rect(640, 0, 640, 1080), Rect(1280, 0, 640, 1080)]
    )
    checks = Checks()
    tiling.run(platform, checks)
    assert any("overlap" in f for f in checks.failures)


def test_a_window_off_the_screen_fails() -> None:
    platform = FakePlatform(
        [Rect(0, 0, 640, 1080), Rect(640, 0, 640, 1080), Rect(1500, 0, 640, 1080)]
    )
    checks = Checks()
    tiling.run(platform, checks)
    assert any("inside the work area" in f for f in checks.failures)
