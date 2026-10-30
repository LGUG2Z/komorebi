"""KDE as a [`Platform`](komorebi_vm.core.platform.Platform): a `Kde` session, seen through the
neutral vocabulary."""

import json
import shlex
from typing import Any

from PIL import Image

from komorebi_vm.core import snapshot
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow, Predicate
from komorebi_vm.core.platform import Pointer as PointerDevice
from komorebi_vm.core.snapshot import Snapshot
from komorebi_vm.kde import Kde

# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24
# the desktop that komorebi puts the windows it hides on, which is the last one
PARK = "komorebi-park"


class KdePlatform:
    """The platform of a headless KWin session of the test machine."""

    def __init__(
        self, kde: Kde, size: tuple[int, int], pointer: PointerDevice | None = None
    ) -> None:
        self.kde = kde
        self.size = size
        self._pointer = pointer

    def has(self, feature: str) -> bool:
        return feature == "pointer" and self._pointer is not None

    @property
    def pointer(self) -> PointerDevice:
        if self._pointer is None:
            raise AssertionError("this session has no pointer device")
        return self._pointer

    @property
    def tolerance(self) -> int:
        return CELL

    def wait_for(self, predicate: Predicate, timeout: float = 10) -> bool:
        return self.kde.wait_for(predicate, timeout=timeout)

    def start_komorebi(self) -> None:
        self.kde.start_komorebi()
        status = self.kde.wait_for(lambda: self.kde.run("komorebic state")[0] == 0, timeout=30)
        if not status:
            raise AssertionError("the daemon did not answer komorebic")

    def restart_with(self, config: dict[str, Any]) -> None:
        if self.kde.komorebi_running():
            self.kde.komorebic("stop")
            self.kde.wait_for(lambda: not self.kde.komorebi_running(), timeout=30)
        path = "/tmp/e2e-restart-config.json"
        self.kde.sh(f"printf %s {shlex.quote(json.dumps(config))} > {path}")
        self.kde.start_komorebi("--config", path)
        if not self.kde.wait_for(lambda: self.kde.run("komorebic state")[0] == 0, timeout=30):
            raise AssertionError("the daemon did not answer komorebic")

    def komorebic(self, *args: str) -> str:
        return self.kde.komorebic(*args)

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        return self.kde.run(shlex.join(["komorebic", *args]))

    def state(self) -> Snapshot:
        return snapshot.parse(self.kde.state())

    def spawn(self, title: str) -> Client:
        self.kde.spawn(f"foot -T {shlex.quote(title)} sleep 3000")
        self.kde.wait_for(lambda: self.kde.wid(title) != "", timeout=30)
        return Client(title)

    def os_window(self, client: Client) -> OsWindow | None:
        wid = self.kde.wid(client.title)
        if wid == "":
            return None
        return OsWindow(client.title, self.kde.rect(wid), self.kde.active() == wid)

    def hidden(self, client: Client) -> bool:
        """The window is on the parking desktop, or on a desktop that is not the one in view.

        KDE keeps the windows of a workspace on a virtual desktop of its own and parks the ones a
        stack or a monocle puts away, so both are out of sight.
        """
        wid = self.kde.wid(client.title)
        if wid == "":
            return False
        desk = self.kde.desk(wid)
        names = self.kde.desktop_names()
        if PARK in names and desk == names.index(PARK) + 1:
            return True
        return desk != 0 and desk != self.kde.shown()

    def displays(self) -> list[Rect]:
        """The enabled outputs, which this session gives the size it was started with, side by side
        from the left (at the scale of 1 that a scenario leaves them at)."""
        width, height = self.size
        return [Rect(i * width, 0, width, height) for i in range(len(self.kde.outputs.names()))]

    def add_display(self) -> None:
        """Enables an output that was disabled, which is as near to a plug as KWin gets."""
        disabled = self.kde.outputs.names(enabled=False)
        if not disabled:
            raise AssertionError("every output is plugged in")
        self.kde.outputs.enable(disabled[0])
        self.kde.wait_for(lambda: self.kde.outputs() == len(self.kde.outputs.names()), timeout=20)

    def remove_display(self) -> None:
        """Disables the last of the enabled outputs, which is KWin's way to unplug one."""
        names = self.kde.outputs.names()
        if len(names) < 2:
            raise AssertionError("there is only one output to remove")
        self.kde.outputs.disable(names[-1])
        self.kde.wait_for(lambda: len(self.kde.outputs.names()) == len(names) - 1, timeout=20)

    def close(self, client: Client) -> None:
        self.kde.close(self.kde.wid(client.title))

    def work_area(self) -> Rect:
        return Rect(0, 0, *self.size)

    def minimize(self, client: Client) -> None:
        self.kde.minimize(self.kde.wid(client.title))

    def activate(self, client: Client) -> None:
        self.kde.activate(self.kde.wid(client.title))

    def screenshot(self, name: str) -> Image.Image:
        return self.kde.shot(name)
