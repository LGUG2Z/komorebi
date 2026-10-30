"""KDE as a [`Platform`](komorebi_vm.core.platform.Platform): a `Kde` session, seen through the
neutral vocabulary."""

import json
import re
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
# the file the daemon is started with, and where a program with a name of its own is made
CONFIG = "/tmp/e2e-restart-config.json"
BIN = "/tmp/e2e-bin"
# what a program name may be made of: the same on every platform, so that a scenario cannot pass
# one that another port rejects
PROGRAM = re.compile(r"[A-Za-z0-9_-]+")


class KdePlatform:
    """The platform of a headless KWin session of the test machine."""

    def __init__(
        self,
        kde: Kde,
        size: tuple[int, int],
        pointer: PointerDevice | None = None,
        session: bool = False,
    ) -> None:
        """`session` is for a real Plasma session, which has a lock screen; the headless KWin of
        the other suites has none."""
        self.kde = kde
        self.session = session
        self.size = size
        self._pointer = pointer

    def has(self, feature: str) -> bool:
        if feature == "lock":
            return self.session
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
        # a unit that is still stopping is not started again by `systemctl start`
        self.kde.wait_for(lambda: not self.kde.komorebi_running(), timeout=30)
        self.kde.start_komorebi()
        status = self.kde.wait_for(lambda: self.kde.run("komorebic state")[0] == 0, timeout=30)
        if not status:
            tail = "\n".join(self.kde.komorebi_log().splitlines()[-30:])
            raise AssertionError(f"the daemon did not answer komorebic; its log ends:\n{tail}")

    def restart_with(self, config: dict[str, Any]) -> None:
        if self.kde.komorebi_running():
            self.kde.komorebic("stop")
            self.kde.wait_for(lambda: not self.kde.komorebi_running(), timeout=30)
        self.rewrite_config(config)
        self.kde.start_komorebi("--config", CONFIG)
        if not self.kde.wait_for(lambda: self.kde.run("komorebic state")[0] == 0, timeout=30):
            raise AssertionError("the daemon did not answer komorebic")

    def rewrite_config(self, config: dict[str, Any]) -> None:
        self.kde.sh(f"printf %s {shlex.quote(json.dumps(config))} > {CONFIG}")

    def komorebic(self, *args: str) -> str:
        return self.kde.komorebic(*args)

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        return self.kde.run(shlex.join(["komorebic", *args]))

    def state(self) -> Snapshot:
        return snapshot.parse(self.kde.state())

    def spawn(
        self,
        title: str,
        app_id: str | None = None,
        exe: str | None = None,
        rename_to: str | None = None,
        after: float = 0,
    ) -> Client:
        name = f"-a {shlex.quote(app_id)} " if app_id else ""
        program = "foot"
        if exe is not None:
            if not PROGRAM.fullmatch(exe):
                raise ValueError(f"not a program name: {exe!r}")
            # the daemon reads the executable from /proc, where a link shows its target: a copy
            # of the terminal is what has the name, made once: a copy that is running cannot be
            # written over
            program = f"{BIN}/{exe}"
            self.kde.sh(
                f"mkdir -p {BIN} && {{ test -x {program} || cp -L $(command -v foot) {program}; }}"
            )
        command = "sleep 3000"
        if rename_to is not None:
            # foot takes the title from the terminal escape OSC 0
            script = f'sleep {after}; printf "\\033]0;{rename_to}\\007"; sleep 3000'
            command = f"sh -c {shlex.quote(script)}"
        self.kde.spawn(f"{program} {name}-T {shlex.quote(title)} {command}")
        if not self.kde.wait_for(lambda: self.kde.wid(title) != "", timeout=30):
            raise AssertionError(f"no window titled {title!r} appeared")
        return Client(title)

    def _wid(self, client: Client) -> str:
        """The id of the window of `client`, which has to exist: a command for a window that is
        not there fails, as it does on every platform."""
        wid = self.kde.wid(client.title)
        if wid == "":
            raise AssertionError(f"no window titled {client.title!r}")
        return wid

    def exe_id(self, program: str) -> str:
        return program

    def os_window(self, client: Client) -> OsWindow | None:
        wid = self.kde.wid(client.title)
        if wid == "":
            return None
        return OsWindow(client.title, self.kde.rect(wid), self.kde.active() == wid)

    def hidden(self, client: Client) -> bool:
        """The window is on the parking desktop, or on a desktop that is not the one in view.

        KDE keeps the windows of a workspace on a virtual desktop of its own and parks the ones a
        stack or a monocle puts away, so both are out of sight. That is every window that is on a
        desktop that is not shown, whoever put it there: this is the quirk
        `Platform.hidden` warns of. With more than one output, the desktop in view is that of
        the output KWin considers active, so only the windows of the first are certain.
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
        """The enabled outputs, as KWin has them, left to right.

        Where KWin does not say, they are the size the session was started with, side by side
        from the left (at the scale of 1 that a scenario leaves them at).
        """
        screens = self.kde.screens()
        if screens:
            return [display for display, _ in screens]
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
        self.kde.close(self._wid(client))

    def _session_id(self) -> str:
        """The id of the user's Wayland session, which is the one that gets locked."""
        for line in self.kde.sh("loginctl list-sessions --no-legend").splitlines():
            words = line.split()
            if len(words) > 2 and words[2] == self.kde.user:
                shown = self.kde.sh(f"loginctl show-session {words[0]} -p Type --value")
                if shown.strip() == "wayland":
                    return words[0]
        raise AssertionError("the user has no Wayland session")

    def _locked(self, locked: bool) -> bool:
        shown = self.kde.sh(f"loginctl show-session {self._session_id()} -p LockedHint --value")
        return (shown.strip() == "yes") == locked

    def lock(self) -> None:
        self.kde.sh(f"loginctl lock-session {self._session_id()}")
        if not self.kde.wait_for(lambda: self._locked(True), timeout=30):
            raise AssertionError("the screen did not lock")

    def unlock(self) -> None:
        self.kde.sh(f"loginctl unlock-session {self._session_id()}")
        if not self.kde.wait_for(lambda: self._locked(False), timeout=30):
            raise AssertionError("the screen did not unlock")

    def move_window(self, client: Client, x: int, y: int) -> None:
        self.kde.sh(f"kdotool windowmove {self._wid(client)} {x} {y}")

    def work_area(self) -> Rect:
        """What KWin says windows may take of the first output, which is the output less the
        panels and docks the shell has made (a headless session has none, so it is all of it).

        Where KWin does not answer, the first output whole: a scenario then checks less, and does
        not fail for it.
        """
        area = self.kde.work_area()
        return area if area is not None else Rect(0, 0, *self.size)

    def minimize(self, client: Client) -> None:
        self.kde.minimize(self._wid(client))

    def activate(self, client: Client) -> None:
        self.kde.activate(self._wid(client))

    def screenshot(self, name: str) -> Image.Image:
        return self.kde.shot(name)
