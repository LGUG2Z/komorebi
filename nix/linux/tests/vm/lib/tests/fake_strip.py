"""A fake whose scrolling layout is a strip of columns, for the shared `scrolling.columns`.

Every window is a container of the workspace, as the daemon has it; the columns the strip lays
out are what this class adds: the windows of each, whether the column is stacked, its width, and
how far the strip is scrolled.
"""

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow
from tests.fake_platform import FakePlatform

REST = 5


class FakeStrip(FakePlatform):
    def __init__(self, *args, **kwargs) -> None:  # noqa: ANN002, ANN003
        super().__init__(*args, **kwargs)
        self.cols: list[list[str]] = []
        self.stacked: set[int] = set()
        self.wide: set[int] = set()
        self.per_view = 2
        self.offset = 0
        self.leaks = False

    # the geometry of the strip
    def _scrolls(self) -> bool:
        return self.layout_name == "Scrolling"

    def _width(self, i: int) -> int:
        w = self.area.w
        return w if i in self.wide else w // self.per_view

    def _left(self, i: int) -> int:
        return sum(self._width(j) for j in range(i))

    def _column_of(self, title: str) -> int:
        return next(i for i, col in enumerate(self.cols) if title in col)

    def _title_in_focus(self) -> str:
        return self.containers[self.focus][0]

    def _keep_in_view(self) -> None:
        i = self._column_of(self._title_in_focus())
        left, right = self._left(i), self._left(i) + self._width(i)
        if left < self.offset:
            self.offset = left
        elif right > self.offset + self.area.w:
            self.offset = right - self.area.w

    def _focus_column(self, i: int) -> None:
        title = self.cols[i][0]
        self.focus = next(k for k, c in enumerate(self.containers) if c[0] == title)

    def _strip_frame(self, title: str) -> Rect:
        i = self._column_of(title)
        x, w = self._left(i) - self.offset, self._width(i)
        if x + w <= 0:
            x = REST - w
        elif x >= self.area.w:
            x = self.area.w - REST
        if self._next_door() and x < self.area.w < x + w:
            w = self.area.w - x
        col = self.cols[i]
        if len(col) == 1 or i in self.stacked:
            return Rect(x, 0, w, self.area.h)
        half = self.area.h // len(col)
        return Rect(x, col.index(title) * half, w, half)

    def _next_door(self) -> bool:
        """Another display is next to the right edge, and the daemon looks after it."""
        return self.display_count > 1 and not self.leaks

    # what the windows are and do
    def spawn(self, title, *args, **kwargs) -> Client:  # noqa: ANN001, ANN002, ANN003
        client = super().spawn(title, *args, **kwargs)
        if title in [c[0] for c in self.containers]:
            self.cols.append([title])
            self.focus = len(self.containers) - 1
            self._keep_in_view()
        return client

    def close(self, client: Client) -> None:
        super().close(client)
        self.cols = [[t for t in col if t != client.title] for col in self.cols]
        self.cols = [col for col in self.cols if col]
        self.stacked = set()
        self.wide = set()

    def os_window(self, client: Client) -> OsWindow | None:
        window = super().os_window(client)
        if window is None or not self._scrolls() or client.title not in sum(self.cols, []):
            return window
        return OsWindow(window.title, self._strip_frame(client.title), window.focused)

    def hidden(self, client: Client) -> bool:
        if self._scrolls() and client.title in sum(self.cols, []):
            i = self._column_of(client.title)
            if self._next_door() and self._strip_frame(client.title).x >= self.area.w - REST:
                return True
            return i in self.stacked and client.title != self._title_in_focus()
        return super().hidden(client)

    def komorebic(self, *args: str) -> str:
        if args[0] in ("focus-monitor", "focus-workspace"):
            return ""
        if not self._scrolls() and args[0] != "change-layout":
            return super().komorebic(*args)
        i = self._column_of(self._title_in_focus()) if self.cols and self._scrolls() else 0
        match args:
            case ("focus", "left" | "right" as way):
                self._focus_column(
                    max(0, min(i + (1 if way == "right" else -1), len(self.cols) - 1))
                )
                self._keep_in_view()
            case ("scrolling-layout-columns", n):
                self.per_view = int(n)
            case ("pan", way):
                step = self._width(0) * (1 if way == "right" else -1)
                limit = max(0, self._left(len(self.cols)) - self.area.w)
                self.offset = max(0, min(self.offset + step, limit))
                shown = [
                    j
                    for j in range(len(self.cols))
                    if self._left(j) >= self.offset
                    and self._left(j) + self._width(j) <= self.offset + self.area.w
                ]
                if i not in shown:
                    self._focus_column(min(shown, key=lambda j: abs(j - i)))
            case ("consume", "left") if i > 0:
                self.cols[i - 1 : i + 1] = [self.cols[i - 1] + self.cols[i]]
                self.stacked = set()
                self.wide = set()
            case ("expel",) if len(self.cols[i]) > 1:
                title = self._title_in_focus()
                self.cols[i].remove(title)
                self.cols.insert(i + 1, [title])
                self.stacked = set()
                self.wide = set()
                self._focus_column(i + 1)
                self._keep_in_view()
            case ("toggle-column-stack",):
                self.stacked ^= {i}
            case ("scrolling-layout-full-width",):
                self.wide ^= {i}
                self._keep_in_view()
            case ("change-layout", name):
                self.layout_name = "BSP" if name == "bsp" else name.title()
            case _:
                return super().komorebic(*args)
        return ""
