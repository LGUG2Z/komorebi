"""A fake that sets wallpapers from the configuration it is started with, shows them on its
screenshots, and draws a ring around the focused window in a hue of the wallpaper."""

from typing import Any

from PIL import Image, ImageDraw

from komorebi_vm.core.platform import Client
from tests.fake_platform import FakePlatform

HUES = {"warm": (230, 40, 40), "cool": (40, 60, 230), "other": (60, 230, 60)}


# the ring is the wallpaper's hue outside the window, which is the color of what is inside it
class FakeWallpaper(FakePlatform):
    def __init__(self, *args, **kwargs) -> None:  # noqa: ANN002, ANN003
        super().__init__(*args, **kwargs)
        self.shown = ""
        self.monitor_wallpaper = ""
        self.workspace_wallpapers: dict[int, str] = {}
        self.workspace = 0
        self.puts_back = True
        # room outside the first window for its ring
        self.workspace_padding = 10

    def put_file(self, path: str, data: bytes) -> None:
        assert data.startswith(b"\x89PNG"), "a picture"

    def wallpaper(self, display: int = 0) -> str:
        return self.shown

    def set_wallpaper(self, path: str, display: int = 0) -> None:
        self.shown = path

    def restart_with(self, config: dict[str, Any]) -> None:
        self.running = True
        monitor = config["monitors"][0]
        self.monitor_wallpaper = monitor["wallpaper"]["path"]
        self.workspace_wallpapers = {
            i: w["wallpaper"]["path"]
            for i, w in enumerate(monitor["workspaces"])
            if "wallpaper" in w
        }
        self._show()

    def _show(self) -> None:
        self.shown = self.workspace_wallpapers.get(self.workspace, self.monitor_wallpaper)

    def komorebic(self, *args: str) -> str:
        if args[0] == "focus-workspace":
            self.workspace = self.current = int(args[1])
            self.focus = min(self.focus, max(0, len(self.containers) - 1))
            if self.puts_back or self.shown != self.wallpaper():
                self._show()
            return ""
        return super().komorebic(*args)

    def screenshot(self, name: str) -> Image.Image:
        which = "other" if "other" in self.shown else "cool" if "cool" in self.shown else "warm"
        image = Image.new("RGB", (1920, 1080), HUES[which])
        draw = ImageDraw.Draw(image)
        for client in (Client(t) for c in self.containers for t in c):
            window = self.os_window(client)
            if window is not None and window.focused:
                f = window.frame
                draw.rectangle((f.x, f.y, f.x + f.w, f.y + f.h), fill=(48, 48, 48))
        return image
