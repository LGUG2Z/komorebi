"""The Windows guest as a `Platform` for the shared scenarios.

Draft: written against the `Platform` protocol of kfde's `komorebi_vm.core`
and not run yet, because the guest agent is not built. The import path moves
with `core/` into this tree.

Hands and eyes split the way the plan has them: keys, pointer and screenshots
go through QMP, on the host; windows, processes and files go through the agent
in the guest's interactive session.
"""

import json
import re
import time
from pathlib import Path
from typing import Any

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, OsWindow, Predicate
from komorebi_vm.core.snapshot import Snapshot, parse

from .agent import Agent, AgentError
from .guest import HOTPLUG_PORTS
from .qmp import Qmp

# where the host driver puts the binaries under test, in the guest
GUEST_BIN = "C:\\e2e\\bin"
# komorebi.exe's stdout and stderr, kept for a launch that fails
DAEMON_OUTPUT = "C:\\e2e\\komorebi.out"
# what the wrapper writes after the daemon, with its exit code
EXIT_LINE = "komorebi exit code"
# colour codes in the daemon's output
ANSI = re.compile(r"\x1b\[[0-9;]*m")
# a daemon that exited at start is launched again, this many times in all
LAUNCHES = 3
# the static configuration `restart_with` writes
POLICY_SYSTEM = "HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\System"
# the account the image logs in to
PASSWORD = "komorebi"
DISPLAY_SCRIPT = "C:\\e2e\\display.ps1"
CONFIG_PATH = "C:\\e2e\\komorebi.json"


class WindowsPointer:
    """The pointer, driven through QMP's absolute tablet on the host.

    The tablet spans the first display, so a pixel of it is a pixel of the
    first monitor.
    """

    def __init__(self, qmp: Qmp, agent: Agent) -> None:
        self._qmp = qmp
        self._agent = agent

    def _size(self) -> tuple[int, int]:
        monitor = self._agent.displays()[0]["monitor"]
        return monitor["right"], monitor["bottom"]

    def move(self, x: int, y: int) -> None:
        self._qmp.pointer_move(x, y, *self._size())

    def down(self) -> None:
        self._qmp.pointer_button(True)

    def up(self) -> None:
        self._qmp.pointer_button(False)

    def drag(
        self,
        start: tuple[int, int],
        end: tuple[int, int],
        steps: int = 25,
        ms: int = 30,
    ) -> None:
        width, height = self._size()
        self._qmp.pointer_move(*start, width, height)
        time.sleep(0.2)
        self._qmp.pointer_button(True)
        for i in range(1, steps + 1):
            x = start[0] + (end[0] - start[0]) * i // steps
            y = start[1] + (end[1] - start[1]) * i // steps
            self._qmp.pointer_move(x, y, width, height)
            time.sleep(ms / 1000)
        self._qmp.pointer_button(False)


class WindowsPlatform:
    """One Windows guest: a QEMU instance, its agent, and the daemon in it."""

    # the DWM frame is the visible frame, so this is only for rounding
    tolerance = 2

    def __init__(
        self, qmp: Qmp, agent: Agent, out: Path, scale: float = 1.0
    ) -> None:
        self._qmp = qmp
        self._agent = agent
        self._out = out
        self._scale = scale
        self._display_script = False
        self._daemon_pid: int | None = None
        self.start_notes: list[str] = []
        self._pointer = WindowsPointer(qmp, agent)
        self._added: list[str] = []
        self._ports_used = 0

    def has(self, feature: str) -> bool:
        return feature in ("pointer", "lock")

    @property
    def pointer(self) -> WindowsPointer:
        return self._pointer

    def wait_for(self, predicate: Predicate, timeout: float = 10) -> bool:
        deadline = time.monotonic() + timeout * self._scale
        while time.monotonic() < deadline:
            try:
                if predicate():
                    return True
            except RuntimeError:
                # `komorebic` failed or timed out while the host is busy:
                # not yet, and the last call below says if it still fails
                pass
            time.sleep(0.1)
        return predicate()

    def start_komorebi(
        self, log_level: str | None = None, config: str | None = None
    ) -> None:
        """Starts the daemon and waits until it answers.

        komorebi can fail to launch (`AllowSetForegroundWindow` failing five
        times at once, for one), and launching it again works, so a daemon
        that has exited without answering is started again, up to
        `LAUNCHES` times in all. Every launch that failed is kept, with what
        the daemon wrote to stderr, in `start_notes`.
        """
        # kfw applies the last state file at start unless told not to, so
        # every scenario starts clean
        args = ["--clean-state"]
        if config:
            args += ["--config", config]
        if log_level:
            args += ["--log-level", log_level]
        command = " ".join([f"{GUEST_BIN}\\komorebi.exe", *args])
        for launch in range(1, LAUNCHES + 1):
            # through cmd, to keep what the daemon prints if it dies
            # `!errorlevel!` is the daemon's exit code, said when it ends
            self._daemon_pid = self._agent.spawn(
                "cmd", "/v:on", "/c",
                f"{command} > {DAEMON_OUTPUT} 2>&1 & "
                f"echo {EXIT_LINE} !errorlevel! >> {DAEMON_OUTPUT}",
            )
            outcome = self._wait_launched()
            if outcome is None:
                return
            self.start_notes.append(f"launch {launch}: {outcome}")
            print(f"     komorebi: launch {launch} failed: {outcome}")
        raise RuntimeError(
            "komorebi did not answer `komorebic state` after "
            f"{LAUNCHES} launches: {'; '.join(self.start_notes[-LAUNCHES:])}"
        )

    def _wait_launched(self) -> str | None:
        """None once the daemon answers; otherwise why this launch failed:
        it ended (with its exit code and what it printed), or it ran on and
        never answered.

        That it ended is known from the exit code the wrapper in
        `start_komorebi` writes after it; a process list read right after
        the launch races the process coming into being.
        """
        deadline = time.monotonic() + 60 * self._scale
        while time.monotonic() < deadline:
            try:
                self.komorebic("state")
                return None
            except Exception:
                pass
            try:
                text = self._agent.get_file(DAEMON_OUTPUT).decode(
                    errors="replace"
                )
            except AgentError:
                text = ""
            text = ANSI.sub("", text)
            lines = [line for line in text.splitlines() if line.strip()]
            if any(line.startswith(EXIT_LINE) for line in lines):
                # an eyre report starts at "Error:", and the backtrace after
                # it is noise; otherwise the end of the output counts
                start = next(
                    (i for i, line in enumerate(lines)
                     if line.startswith("Error")), None
                )
                shown = lines[start:start + 3] if start is not None \
                    else lines[-4:]
                return "ended without answering; it printed: " + " | ".join(
                    line.strip() for line in shown
                )
            time.sleep(0.5)
        return "ran for 60 s and did not answer"

    def restart_with(self, config: dict[str, Any]) -> None:
        """Stops the daemon if it runs and starts it from a clean state with
        `config` as its static configuration; open windows are taken up."""
        if self.komorebic_status("state")[0] == 0:
            self.komorebic("stop")
            if not self.wait_for(
                lambda: self.komorebic_status("state")[0] != 0, timeout=20
            ):
                raise RuntimeError("komorebi did not stop")
        local = self._out / "komorebi.json"
        local.write_text(json.dumps(config, indent=2))
        self._agent.put_file(local, CONFIG_PATH)
        self.start_komorebi(config=CONFIG_PATH)

    def rewrite_config(self, config: dict) -> None:
        local = self._out / "komorebi.json"
        local.write_text(json.dumps(config, indent=2))
        # the daemon watches the file it was started with and reads it again
        self._agent.put_file(local, CONFIG_PATH)

    def resizable_display(self) -> int | None:
        """The index of a display whose mode can change: the second, which is a
        QXL display. The first is the basic display adapter, with one mode, and
        there is no second until one is plugged in."""
        return 1 if len(self.displays()) > 1 else None

    def resize_display(self, index: int, width: int, height: int) -> None:
        """Changes the mode of display `index` and returns when the operating
        system has it. Only a display of a QXL device lists modes: the first
        one is the basic display adapter, which has the one."""
        if not self._display_script:
            self._agent.exec("cmd", "/c", "mkdir C:\\e2e 2>nul")
            self._agent.put_file(
                Path(__file__).parent / "display.ps1", DISPLAY_SCRIPT
            )
            self._display_script = True
        result = self._agent.exec(
            "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass",
            "-File", DISPLAY_SCRIPT, "set", f"\\\\.\\DISPLAY{index + 1}",
            str(width), str(height),
        )
        if not result["stdout"].strip().endswith(": 0"):
            raise RuntimeError(f"could not change the display: {result}")
        if not self.wait_for(
            lambda: (d := self.displays())[index].w == width
            and d[index].h == height
        ):
            raise RuntimeError("the display did not take the mode")

    def exe_id(self, program: str) -> str:
        # kfw names the exe of a window with its extension
        return f"{program}.exe"

    def _locked(self) -> bool:
        listing = self._agent.exec(
            "tasklist", "/fi", "imagename eq LogonUI.exe"
        )["stdout"]
        return "LogonUI.exe" in listing

    def lock(self) -> None:
        # the image forbids locking (it has no use for a lock screen): allow
        # it for this guest
        self._agent.exec(
            "reg", "add", POLICY_SYSTEM, "/v", "DisableLockWorkstation",
            "/t", "REG_DWORD", "/d", "0", "/f",
        )
        self._agent.exec("rundll32.exe", "user32.dll,LockWorkStation")
        if not self.wait_for(self._locked, timeout=20):
            raise RuntimeError("the screen did not lock")

    def unlock(self) -> None:
        # a key to wake the sign-in screen, then the password of the account
        # the image logs in to (autounattend.xml)
        self._qmp.key("shift")
        time.sleep(2 * self._scale)
        for ch in PASSWORD:
            self._qmp.key(ch)
        self._qmp.key("ret")
        if not self.wait_for(lambda: not self._locked(), timeout=30):
            raise RuntimeError("the screen did not unlock")

    def move_window(self, client: Client, x: int, y: int) -> None:
        window = self._find(client.title)
        if window is None:
            raise RuntimeError(f"no window titled {client.title!r}")

        def set_position(left: int, top: int) -> None:
            # SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE
            result = self._agent.exec(
                "powershell", "-NoProfile", "-Command",
                "Add-Type -Name W -Namespace K -MemberDefinition "
                "'[DllImport(\"user32.dll\")] public static extern bool "
                "SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, "
                "int cy, uint f);'; "
                f"[K.W]::SetWindowPos([IntPtr]{window['hwnd']}, "
                f"[IntPtr]::Zero, {left}, {top}, 0, 0, 0x15)",
            )
            if "True" not in result["stdout"]:
                raise RuntimeError(f"could not move {client.title!r}: {result}")

        set_position(int(x), int(y))
        # the frame the OS reports leaves out the invisible border of the
        # window: move again by what is left to the corner asked for
        moved = self.os_window(client)
        if moved is not None and (moved.frame.x, moved.frame.y) != (x, y):
            f = moved.frame
            set_position(int(x) - (f.x - int(x)), int(y) - (f.y - int(y)))

    def _wait_answering(self) -> None:
        def answers() -> bool:
            try:
                self.komorebic("state")
            except Exception:
                return False
            return True

        if not self.wait_for(answers, timeout=60):
            raise RuntimeError("komorebi did not answer `komorebic state`")

    def komorebic(self, *args: str) -> str:
        result = self._agent.exec(f"{GUEST_BIN}\\komorebic.exe", *args)
        if result["status"] != 0:
            raise RuntimeError(
                f"komorebic {' '.join(args)}: {result['stderr'] or result}"
            )
        return result["stdout"]

    def state(self) -> Snapshot:
        return parse(json.loads(self.komorebic("state")))

    def spawn(
        self, title: str, app_id: str | None = None, exe: str | None = None
    ) -> Client:
        args = ["--title", title]
        if app_id is not None:
            args += ["--class", app_id]
        program = f"{GUEST_BIN}\\komorebi-testwin.exe"
        if exe is not None:
            if not re.fullmatch(r"[A-Za-z0-9_-]+", exe):
                raise ValueError(f"not a program name: {exe!r}")
            # a program of that name, for a rule that matches by it
            program = f"{GUEST_BIN}\\{exe}.exe"
            self._agent.exec(
                "cmd", "/c", f"copy /y {GUEST_BIN}\\komorebi-testwin.exe {program}"
            )
        self._agent.spawn(program, *args)
        if not self.wait_for(lambda: self._find(title) is not None):
            raise RuntimeError(f"no window titled {title!r} appeared")
        return Client(title)

    def _find(self, title: str) -> dict | None:
        for window in self._agent.os_windows():
            if window["title"] == title:
                return window
        return None

    def os_window_by_title(self, title: str) -> dict | None:
        return self._find(title)

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
        # komorebi hides a window of another workspace by cloaking it
        window = self._find(client.title)
        return window is not None and window["cloaked"]

    def close(self, client: Client) -> None:
        window = self._find(client.title)
        if window is None:
            raise RuntimeError(f"no window titled {client.title!r}")
        self._agent.call("close", hwnd=window["hwnd"])

    def komorebic_status(self, *args: str) -> tuple[int, str]:
        result = self._agent.exec(f"{GUEST_BIN}\\komorebic.exe", *args)
        return result["status"], result["stdout"] + result["stderr"]

    def _displays(self) -> list[dict]:
        # the agent may drop the connection while the display topology
        # changes; one reconnect is enough
        try:
            return self._agent.displays()
        except (OSError, ValueError, AgentError):
            self._agent.connect(timeout=10)
            return self._agent.displays()

    def displays(self) -> list[Rect]:
        monitors = [d["monitor"] for d in self._displays()]
        rects = [
            Rect(m["left"], m["top"], m["right"], m["bottom"])
            for m in monitors
        ]
        return sorted(rects, key=lambda r: r.x)

    def _wait_displays(self, count: int) -> None:
        if not self.wait_for(lambda: len(self.displays()) == count, 60):
            raise RuntimeError(f"the OS did not reach {count} displays")

    def add_display(self) -> None:
        # a port is never reused: QEMU aborts when a QXL device is added at a
        # path whose RAM blocks the one before it has not given back yet
        self._ports_used += 1
        n = self._ports_used
        if n > HOTPLUG_PORTS:
            raise RuntimeError("no hotplug port left for another display")
        before = len(self.displays())
        self._qmp.execute(
            "device_add", driver="qxl", id=f"qxl{n}", bus=f"rp{n}",
            ram_size_mb=64, vram_size_mb=64, vgamem_mb=16,
        )
        self._added.append(f"qxl{n}")
        self._wait_displays(before + 1)

    def remove_display(self) -> None:
        if not self._added:
            raise RuntimeError("no display was added by add_display")
        before = len(self.displays())
        self._qmp.execute("device_del", id=self._added.pop())
        self._wait_displays(before - 1)

    def work_area(self) -> Rect:
        area = self._agent.displays()[0]["work_area"]
        return Rect(area["left"], area["top"], area["right"], area["bottom"])

    def activate(self, client: Client) -> None:
        window = self._find(client.title)
        if window is None:
            raise RuntimeError(f"no window titled {client.title!r}")
        self._agent.call("activate", hwnd=window["hwnd"])

    def minimize(self, client: Client) -> None:
        window = self._find(client.title)
        if window is None:
            raise RuntimeError(f"no window titled {client.title!r}")
        self._agent.call("minimize", hwnd=window["hwnd"])

    def screenshot(self, name: str) -> Image.Image:
        path = self._out / f"{name}.png"
        self._qmp.screendump(path)
        return Image.open(path)

    def agent_exec(self, *args: str) -> dict:
        """Runs a program in the guest, for the recorder step of the runner."""
        return self._agent.exec(*args)

    def agent_spawn(self, *args: str) -> int:
        return self._agent.spawn(*args)

    def agent_get_file(self, remote: str) -> bytes:
        return self._agent.get_file(remote)
