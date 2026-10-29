"""KDE as a [`Platform`](komorebi_vm.core.platform.Platform): a `Kde` session, seen through the
neutral vocabulary."""

from PIL import Image

from komorebi_vm.core import snapshot
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow, Predicate
from komorebi_vm.core.snapshot import Snapshot
from komorebi_vm.kde import Kde

# foot sizes itself to whole cells of its font, so its frame can be short of the one asked for
CELL = 24
# the desktop that komorebi puts the windows it hides on, which is the last one
PARK = "komorebi-park"


class KdePlatform:
    """The platform of a headless KWin session of the test machine."""

    def __init__(self, kde: Kde, size: tuple[int, int]) -> None:
        self.kde = kde
        self.size = size

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

    def komorebic(self, *args: str) -> str:
        return self.kde.komorebic(*args)

    def state(self) -> Snapshot:
        return snapshot.parse(self.kde.state())

    def spawn(self, title: str) -> Client:
        self.kde.spawn(f"foot -T {title} sleep 3000")
        self.kde.wait_for(lambda: self.kde.wid(title) != "", timeout=30)
        return Client(title)

    def os_window(self, client: Client) -> OsWindow | None:
        wid = self.kde.wid(client.title)
        if wid == "":
            return None
        return OsWindow(client.title, self.kde.rect(wid), self.kde.active() == wid)

    def hidden(self, client: Client) -> bool:
        names = self.kde.desktop_names()
        wid = self.kde.wid(client.title)
        if PARK not in names or wid == "":
            return False
        return self.kde.desk(wid) == names.index(PARK) + 1

    def work_area(self) -> Rect:
        return Rect(0, 0, *self.size)

    def activate(self, client: Client) -> None:
        self.kde.activate(self.kde.wid(client.title))

    def screenshot(self, name: str) -> Image.Image:
        return self.kde.shot(name)
