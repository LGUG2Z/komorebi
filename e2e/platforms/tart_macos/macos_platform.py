"""The macOS guest as a `Platform` for the shared scenarios.

Everything goes through the agent in the guest (`komorebi-e2e-agent`): the windows and displays
are what the accessibility API says, the daemon is a program the agent starts, and a screenshot is
`screencapture` run by it. That is also where the grants are: the agent is started from the
Terminal that holds Accessibility and Screen Recording, and so are the daemon and the test windows
it starts.

A macOS guest has one display, and nothing here injects input, so what a scenario needs of a second
display, of a pointer, of the lock screen or of the wallpaper is not here: `has` says so, and the
runner leaves those scenarios out with the reason.
"""

import json
import re
import time
from pathlib import Path
from typing import Any

from komorebi_vm.agent import Agent, AgentError
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow, Predicate
from komorebi_vm.core.snapshot import Snapshot, parse
from PIL import Image

from .guest import GUEST_BIN, GUEST_ROOT

DAEMON_OUTPUT = f"{GUEST_ROOT}/komorebi.out"
CONFIG_PATH = f"{GUEST_ROOT}/komorebi.json"
TEMP_DIR = f"{GUEST_ROOT}/tmp"
# colour codes in the daemon's output
ANSI = re.compile(r"\x1b\[[0-9;]*m")
# a daemon that exited at start is launched again, this many times in all
LAUNCHES = 3
# how long after it answers the daemon takes to see a change of its configuration file: it watches
# it with FSEvents, and a file written in the first half second after it started is not noticed (a
# later one is, after two or three seconds). Nobody edits a file that fast, so a start waits this
# long (measured: 0 s loses the change, 0.5 s does not).
WATCH_SETTLE = 1.0
# the commands of `komorebic` that change nothing, which need no barrier after them
READ_ONLY = {"state", "global-state", "query", "visible-windows", "monitor-information"}
# the programs whose windows are not for the daemon to tile: the Terminal the agent runs in, and
# the desktop widgets of the stock image (Notification Center draws them as windows)
IGNORED_APPS = ("Terminal", "Notification Center")
# macOS does not let a window go entirely off the screen, so the daemon parks one with a sliver of it
# on the display (1 px in the corner it hides a window in, 5 px for one that scrolled away): a window
# with no more than this showing in either direction is put away
HIDDEN_SLIVER = 5


def with_ignored_apps(config: dict[str, Any]) -> dict[str, Any]:
    """`config` with a rule that leaves the windows of the guest's own programs alone."""
    merged = dict(config)
    rules = list(merged.get("ignore_rules") or [])
    rules += [
        {"kind": "Exe", "id": app, "matching_strategy": "Equals"} for app in IGNORED_APPS
    ]
    merged["ignore_rules"] = rules
    return merged


def _shows_more_than_a_sliver(frame: Rect, display: Rect) -> bool:
    """Whether more than `HIDDEN_SLIVER` pixels of the window are on the display, in both
    directions: a window that is cut to a thin strip is put away as much as one that is gone."""
    width = min(frame.x + frame.w, display.x + display.w) - max(frame.x, display.x)
    height = min(frame.y + frame.h, display.y + display.h) - max(frame.y, display.y)
    return width > HIDDEN_SLIVER and height > HIDDEN_SLIVER


class MacosPlatform:
    """One macOS guest: its agent, and the daemon in it."""

    # a window's frame is the app's own, with no invisible border: only rounding
    tolerance = 2

    def __init__(self, agent: Agent, out: Path, scale: float = 1.0) -> None:
        self._agent = agent
        self._out = out
        self._scale = scale
        self._daemon_pid: int | None = None
        # the process of each test window, by the title a scenario knows it by
        self._pids: dict[str, int] = {}
        self.start_notes: list[str] = []

    def has(self, feature: str) -> bool:
        # no input is injected, and a guest has no second display and no lock screen to test
        return False

    @property
    def pointer(self) -> Any:
        raise NotImplementedError("a macOS guest has no pointer: has('pointer') is false")

    def wait_for(self, predicate: Predicate, timeout: float = 10) -> bool:
        deadline = time.monotonic() + timeout * self._scale
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return True
            except (RuntimeError, AgentError):
                # `komorebic` failed or timed out while the guest is busy: not yet, and the last
                # call below says if it still fails
                pass
            time.sleep(0.1)
        return predicate()

    # the daemon

    def start_komorebi(
        self, log_level: str | None = None, config: dict[str, Any] | None = None
    ) -> None:
        """Starts the daemon from a clean state and waits until it answers.

        A daemon that has exited without answering is started again, up to `LAUNCHES` times in all.
        Every launch that failed is kept, with what the daemon printed, in `start_notes`."""
        self._write_config(config or {})
        args = ["--config", CONFIG_PATH]
        if log_level:
            args += ["--log-level", log_level]
        for launch in range(1, LAUNCHES + 1):
            self._daemon_pid = self._agent.spawn(
                f"{GUEST_BIN}/komorebi", *args,
                env={"KOMOREBI_CONFIG_HOME": GUEST_ROOT}, out=DAEMON_OUTPUT,
            )
            outcome = self._wait_launched(self._daemon_pid)
            if outcome is None:
                time.sleep(WATCH_SETTLE * self._scale)
                return
            self.start_notes.append(f"launch {launch}: {outcome}")
            print(f"     komorebi: launch {launch} failed: {outcome}")
        raise RuntimeError(
            "komorebi did not answer `komorebic state` after "
            f"{LAUNCHES} launches: {'; '.join(self.start_notes[-LAUNCHES:])}"
        )

    def _wait_launched(self, pid: int) -> str | None:
        """None once the daemon answers; otherwise why this launch failed: it ended (with what it
        printed), or it ran on and never answered."""
        deadline = time.monotonic() + 60 * self._scale
        while time.monotonic() < deadline:
            try:
                self.komorebic("state")
                return None
            except (RuntimeError, AgentError):
                pass
            status = self._agent.status(pid)
            if not status["running"]:
                return (
                    f"ended with {status['code']} without answering; it printed: "
                    + self._daemon_tail()
                )
            time.sleep(0.5)
        return "ran for 60 s and did not answer"

    def _daemon_tail(self) -> str:
        try:
            text = self._agent.get_file(DAEMON_OUTPUT).decode(errors="replace")
        except AgentError:
            return ""
        lines = [line for line in ANSI.sub("", text).splitlines() if line.strip()]
        # an eyre report starts at "Error:", and the backtrace after it is noise
        start = next((i for i, line in enumerate(lines) if line.startswith("Error")), None)
        shown = lines[start:start + 3] if start is not None else lines[-4:]
        return " | ".join(line.strip() for line in shown)

    def _write_config(self, config: dict[str, Any]) -> None:
        local = self._out / "komorebi.json"
        local.write_text(json.dumps(with_ignored_apps(config), indent=2))
        self._agent.put_file(local, CONFIG_PATH)

    def restart_with(self, config: dict[str, Any]) -> None:
        """Stops the daemon if it runs and starts it from a clean state with `config` as its static
        configuration; open windows are taken up."""
        if self.komorebic_status("state")[0] == 0:
            self.komorebic("stop")
            if not self.wait_for(lambda: self.komorebic_status("state")[0] != 0, timeout=20):
                raise RuntimeError("komorebi did not stop")
        self.start_komorebi(config=config)

    def rewrite_config(self, config: dict[str, Any]) -> None:
        # the daemon watches the file it was started with and reads it again
        self._write_config(config)

    def crash(self) -> None:
        # a process that is killed puts nothing back: windows it hid stay where it left them
        assert self._daemon_pid is not None
        self._agent.exec("kill", "-9", str(self._daemon_pid))
        if not self.wait_for(
            lambda: not self._agent.status(self._daemon_pid)["running"], timeout=20
        ):
            raise RuntimeError("komorebi did not go")

    def komorebic(self, *args: str) -> str:
        result = self._agent.exec(f"{GUEST_BIN}/komorebic", *args)
        if result["status"] != 0:
            raise RuntimeError(f"komorebic {' '.join(args)}: {result['stderr'] or result}")
        if args and args[0] not in READ_ONLY:
            self._barrier()
        return result["stdout"]

    def _barrier(self) -> None:
        """Returns once the daemon has taken up what `komorebic` sent it.

        `komorebic` is done when the command is in the daemon's queue, not when it has been
        carried out, and a window that opens right after (the scenarios do that) is an event of
        the same queue: a rule or a name given just before it could come after it. The daemon
        answers `state` in the order it got things, so its answer says the commands before it
        have run. Nothing is asked of Windows, where the daemon is quick enough not to need it."""
        try:
            self._agent.exec(f"{GUEST_BIN}/komorebic", "state")
        except AgentError:
            pass

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        result = self._agent.exec(f"{GUEST_BIN}/komorebic", *args)
        status = result["status"] if result["status"] is not None else -1
        return status, result["stdout"] + result["stderr"]

    def state(self) -> Snapshot:
        printed = self.komorebic("state")
        try:
            return parse(json.loads(printed))
        except ValueError as e:
            # a daemon that is busy can print something else: a wait retries, and a check that
            # keeps failing shows what it was
            raise RuntimeError(
                f"komorebic state did not print JSON ({e}): {printed[:300]!r}"
            ) from e

    # files

    def temp_path(self, name: str) -> str:
        parts = name.replace("\\", "/").split("/")
        if not all(
            re.fullmatch(r"[A-Za-z0-9_.-][A-Za-z0-9 _.-]*", part) and part != ".."
            for part in parts
        ):
            raise ValueError(f"not a file name: {name!r}")
        self._agent.exec("mkdir", "-p", TEMP_DIR)
        return f"{TEMP_DIR}/" + "/".join(parts)

    def put_file(self, path: str, data: bytes) -> None:
        self._agent.exec("mkdir", "-p", path.rsplit("/", 1)[0])
        local = self._out / "put_file.tmp"
        local.write_bytes(data)
        self._agent.put_file(local, path)

    # windows

    def spawn(
        self,
        title: str,
        app_id: str | None = None,
        exe: str | None = None,
        rename_to: str | None = None,
        after: float = 0,
    ) -> Client:
        args = ["--title", title]
        if app_id is not None:
            # a macOS window has no class: the daemon takes its accessibility subrole for one, which
            # the test window is given as this
            args += ["--class", app_id]
        if rename_to is not None:
            # the window gives itself this title `after` seconds from now
            args += ["--rename-to", rename_to, "--after", str(after)]
        program = f"{GUEST_BIN}/komorebi-testwin"
        if exe is not None:
            if not re.fullmatch(r"[A-Za-z0-9_-]+", exe):
                raise ValueError(f"not a program name: {exe!r}")
            # a program of that name, for a rule that matches by it
            program = f"{GUEST_BIN}/{exe}"
            self._agent.exec("cp", f"{GUEST_BIN}/komorebi-testwin", program)
        pid = self._agent.spawn(program, *args)
        self._pids[title] = pid
        if rename_to is not None:
            self._pids[rename_to] = pid
        if not self.wait_for(lambda: self._find(title) is not None):
            raise RuntimeError(f"no window titled {title!r} appeared")
        return Client(rename_to if rename_to is not None else title)

    def exe_id(self, program: str) -> str:
        # the daemon names the program of a window by its file name, which has no extension here
        return program

    def _windows(self) -> list[dict]:
        return self._agent.os_windows()

    def _find(self, title: str) -> dict | None:
        for window in self._windows():
            if window["title"] == title:
                return window
        return None

    def os_window(self, client: Client) -> OsWindow | None:
        window = self._find(client.title)
        if window is None:
            return None
        f = window["frame"]
        return OsWindow(
            title=window["title"],
            frame=Rect(f["left"], f["top"], f["right"], f["bottom"]),
            focused=window["focused"],
        )

    def hidden(self, client: Client) -> bool:
        # the daemon puts a window of another workspace away by moving it to the corner of its
        # display, with a sliver showing (`HIDDEN_SLIVER`); a minimized one is the user's, and one
        # that shows more than that is in view
        window = self._find(client.title)
        if window is None or window["minimized"]:
            return False
        f = window["frame"]
        frame = Rect(f["left"], f["top"], f["right"], f["bottom"])
        return not any(_shows_more_than_a_sliver(frame, d) for d in self.displays())

    def close(self, client: Client) -> None:
        self._agent.call("close", title=client.title)

    def minimize(self, client: Client) -> None:
        self._agent.call("minimize", title=client.title)

    def activate(self, client: Client) -> None:
        self._agent.call("activate", title=client.title)

    def move_window(self, client: Client, x: int, y: int) -> None:
        self._agent.call("move", title=client.title, x=x, y=y)

    def fullscreen(self, client: Client, on: bool = True) -> None:
        # the test window takes itself over its display, borderless, as a program's own key does
        self._agent.send(self._pids[client.title], f"fullscreen {'on' if on else 'off'}")

        def covers() -> bool:
            window = self.os_window(client)
            return window is not None and any(
                window.frame == display for display in self.displays()
            )

        if not self.wait_for(lambda: covers() == on):
            raise RuntimeError(
                f"{client.title!r} did not go {'fullscreen' if on else 'back'}"
            )

    def user_maximize(self, client: Client, on: bool = True) -> None:
        raise NotImplementedError("the zoom button of a window is not driven yet")

    # displays

    def _displays(self) -> list[dict]:
        return self._agent.displays()

    def displays(self) -> list[Rect]:
        rects = []
        for display in self._displays():
            m = display["monitor"]
            rects.append(Rect(m["left"], m["top"], m["right"], m["bottom"]))
        return sorted(rects, key=lambda r: r.x)

    def work_area(self) -> Rect:
        area = self._displays()[0]["work_area"]
        return Rect(area["left"], area["top"], area["right"], area["bottom"])

    def add_display(self) -> None:
        raise NotImplementedError("a macOS guest has one display")

    def remove_display(self) -> None:
        raise NotImplementedError("a macOS guest has one display")

    def resizable_display(self) -> int | None:
        return None

    def resize_display(self, index: int, width: int, height: int) -> None:
        raise NotImplementedError("the display of a macOS guest has the size it was made with")

    # what needs input or a lock screen

    def lock(self) -> None:
        raise NotImplementedError("has('lock') is false")

    def unlock(self) -> None:
        raise NotImplementedError("has('lock') is false")

    def resize_grab(self, frame: Rect, grab: str) -> tuple[int, int]:
        raise NotImplementedError("a macOS guest has no pointer: has('pointer') is false")

    def wallpaper(self, display: int = 0) -> str:
        raise NotImplementedError("the wallpaper of a macOS guest is not read yet")

    def set_wallpaper(self, path: str, display: int = 0) -> None:
        raise NotImplementedError("the wallpaper of a macOS guest is not set yet")

    # what the run keeps

    def screenshot(self, name: str) -> Image.Image:
        remote = f"{TEMP_DIR}/{name}.png"
        self._agent.exec("mkdir", "-p", TEMP_DIR)
        result = self._agent.exec("/usr/sbin/screencapture", "-x", remote)
        if result["status"] != 0:
            raise RuntimeError(f"screencapture: {result['stderr'] or result}")
        path = self._out / f"{name}.png"
        path.write_bytes(self._agent.get_file(remote))
        return Image.open(path)

    def agent_exec(self, *args: str) -> dict:
        """Runs a program in the guest, for the diagnosis of a run."""
        return self._agent.exec(*args)

    def agent_get_file(self, remote: str) -> bytes:
        return self._agent.get_file(remote)
