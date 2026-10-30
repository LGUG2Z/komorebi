"""A KWin session inside a test machine, and the tools to look at it."""

import json
import re
import shlex
import time
from collections.abc import Callable
from itertools import count
from typing import Any

from PIL import Image

from komorebi_vm.core.geometry import Rect
from komorebi_vm.machine import Machine

# a KWin script is told apart by its name in the journal, so each probe has a name of its own
_PROBES = count(1)


class Outputs:
    """The outputs of a KWin session, as `kscreen-doctor` lists and changes them.

    A [`Kde`][] has one, as [`Kde.outputs`][]. Calling it gives the number of outputs KWin has
    enabled. An output is named as KWin names it (`Virtual-0`), and a disabled output keeps its
    name, so that it can be enabled again.
    """

    def __init__(self, kde: "Kde") -> None:
        self._kde = kde

    def __call__(self) -> int:
        """The number of outputs KWin has enabled."""
        return int(
            self._kde.sh("kscreen-doctor -j | jq '[.outputs[] | select(.enabled)] | length'")
        )

    def names(self, *, enabled: bool = True) -> list[str]:
        """The names of the outputs that are enabled (or, with `enabled` false, disabled), from
        left to right."""
        listed = self._kde.sh(
            f"kscreen-doctor -j | jq -r '[.outputs[] | select(.enabled == {str(enabled).lower()})]"
            " | sort_by(.pos.x) | .[].name'"
        )
        return listed.split()

    def disable(self, name: str) -> None:
        """Unplugs an output, as far as KWin can tell: it is disabled, and its windows go to the
        outputs that are left."""
        self._kde.sh(f"kscreen-doctor output.{name}.disable")

    def enable(self, name: str) -> None:
        """Plugs an output back in, as far as KWin can tell."""
        self._kde.sh(f"kscreen-doctor output.{name}.enable")

    def scale(self, name: str, factor: float) -> None:
        """Gives an output another scale: its size in pixels, as the windows see it, becomes
        its mode divided by `factor`."""
        self._kde.sh(f"kscreen-doctor output.{name}.scale.{factor:g}")


class Kde:
    """One KWin session of the test user, with its own session bus and Wayland socket.

    The node runs `kwin_wayland` and the bus as user units (see
    `nix/tests/vm/nodes/headless.nix`), so several sessions can run side by
    side under different names and none touches another. Every command is run
    as the test user, with the environment of the named session.

    A session node has one more: the user's own Plasma session, which
    [`Kde.session`][] attaches to. Its bus is `$XDG_RUNTIME_DIR/bus` and its
    Wayland socket is whatever the user manager says it is.

    `scale` multiplies every wait: 1 under KVM, more where the machine is
    slower (HVF on a Mac, TCG anywhere).
    """

    def __init__(
        self,
        machine: Machine,
        name: str,
        *,
        user: str = "komorebi",
        uid: int = 1000,
        scale: float = 1.0,
        bus: str | None = None,
        wayland_display: str | None = None,
    ) -> None:
        self.machine = machine
        self.name = name
        self.user = user
        self.uid = uid
        self.scale = scale
        self._bus = bus
        self._wayland_display = wayland_display
        self._x_display: str | None = None

    @classmethod
    def headless(
        cls,
        machine: Machine,
        name: str,
        *,
        size: tuple[int, int] = (1920, 1080),
        outputs: int = 1,
        xwayland: bool = False,
        scale: float = 1.0,
        user: str = "komorebi",
        uid: int = 1000,
    ) -> "Kde":
        """Starts `kwin_wayland --virtual` with `outputs` outputs of `size`, and waits for it.

        With `xwayland`, KWin also runs XWayland, for X11 clients. It starts it when the first
        one connects and does not say where, so the session takes the first X11 socket of the
        machine: one session at a time can have XWayland.
        """
        kde = cls(machine, name, user=user, uid=uid, scale=scale)
        machine.succeed(f"loginctl show-user {user} >/dev/null")
        width, height = size
        args = f"--width {width} --height {height} --output-count {outputs}"
        if xwayland:
            args += " --xwayland"
        kde.as_user(f"echo KWIN_ARGS={shlex.quote(args)} > {kde.runtime}/kwin-virtual-{name}.env")
        kde.systemctl("start", f"session-bus@{name}.service", f"kwin-virtual@{name}.service")
        if not kde.wait_for(lambda: kde.has_name("org.kde.KWin"), timeout=60):
            log = machine.execute(f"journalctl _UID={uid} --no-pager -n 40")[1]
            raise AssertionError(f"KWin did not come up:\n{log}")
        if xwayland:
            socket = "ls /tmp/.X11-unix | grep -E '^X[0-9]+$' | sort | head -n 1"
            if not kde.wait_for(lambda: machine.execute(socket)[1].strip() != "", timeout=10):
                raise AssertionError("KWin did not open an X11 socket")
            kde._x_display = ":" + machine.execute(socket)[1].strip()[1:]
        return kde

    @classmethod
    def session(
        cls,
        machine: Machine,
        *,
        user: str = "komorebi",
        uid: int = 1000,
        scale: float = 1.0,
        timeout: float = 300,
    ) -> "Kde":
        """Attaches to the user's Plasma session once its desktop is up.

        The session is up when the shell is running, the user manager knows the
        Wayland socket, the splash screen has gone, and the shell has made its
        panel. The splash screen is the condition a screenshot test depends on:
        it already has thousands of colors on it. The panel is the one a test of
        where a window is depends on: it takes room from the windows, and
        komorebi tiles the room that is left, if it starts after the panel.
        NixOS wraps its programs, so the processes
        are found by their command lines (`pgrep -x` would see only the wrapper's
        name), with the first letter in brackets so that the pattern does not
        match the shell that runs `pgrep`.
        """
        kde = cls(machine, "session", user=user, uid=uid, scale=scale)
        kde._bus = f"unix:path={kde.runtime}/bus"
        if not kde.wait_for(lambda: kde.run("pgrep -f '[p]lasmashell'")[0] == 0, timeout=timeout):
            raise AssertionError("plasmashell did not start")

        def display() -> str:
            out = kde.as_user("systemctl --user show-environment")
            return next(
                (
                    line.split("=", 1)[1]
                    for line in out.splitlines()
                    if line.startswith("WAYLAND_DISPLAY=")
                ),
                "",
            )

        if not kde.wait_for(lambda: display() != "", timeout=timeout):
            raise AssertionError("the user manager never learned the Wayland display")
        kde._wayland_display = display()
        if not kde.wait_for(lambda: kde.run("pgrep -f '[k]splashqml'")[0] != 0, timeout=timeout):
            raise AssertionError("the splash screen did not go away")
        if not kde.wait_for(lambda: kde.panels() > 0, timeout=timeout):
            raise AssertionError("the shell never made a panel")
        return kde

    def panels(self) -> int:
        """Returns how many panels the Plasma shell has made, or 0 when it does not answer.

        A panel takes room from the windows: the work area komorebi tiles is the screen less the
        panel. A test that reads where a tiled window ends has to wait for the panel, or komorebi
        tiles the window under it, and the bottom of a border is hidden by the panel.
        """
        status, out = self.run(
            "busctl --user call org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell"
            " evaluateScript s 'print(panels().length)'"
        )
        found = re.search(r'^s "(\d+)', out.strip())
        return int(found.group(1)) if status == 0 and found else 0

    def panel_has_room(self) -> bool:
        """Returns true if and only if the daemon's work area is not the whole of its first monitor.

        That is the daemon having learned that the panel takes room from the windows. It is not
        true when the daemon does not answer, or answers with something that is not its state.
        """
        try:
            monitor = self.state()["monitors"]["elements"][0]
        except (AssertionError, ValueError, KeyError, IndexError):
            return False
        return monitor["work_area_size"] != monitor["size"]

    def wait_for_panel_room(self, timeout: float = 30) -> bool:
        """Waits until the daemon has the panel's room, and says whether it has.

        The shell makes its panel (`Kde.panels`), KWin takes room for it a while later, and the
        daemon learns of that up to a second after, because the bridge looks at the work area once
        a second. A daemon that tiles before then puts the windows under the panel and tiles them
        again, and a test that reads where a window is, or what is round it, in between, reads
        the wrong tile. The windows are opened after this.

        It is a wait and not a check: a session may never take the room (a machine too slow to
        lay the panel out, which TCG often is), so this ends after `timeout` seconds (times
        `scale`) and the test goes on, whichever it was. The daemon has to be running.
        """
        return self.wait_for(self.panel_has_room, timeout=timeout)

    @property
    def runtime(self) -> str:
        """The test user's runtime directory."""
        return f"/run/user/{self.uid}"

    @property
    def env(self) -> str:
        """The `VAR=value` words that put a command inside this session."""
        profile = f"/etc/profiles/per-user/{self.user}"
        pairs = {
            "XDG_RUNTIME_DIR": self.runtime,
            "DBUS_SESSION_BUS_ADDRESS": self._bus
            or f"unix:path={self.runtime}/session-{self.name}.bus",
            "WAYLAND_DISPLAY": self._wayland_display or f"wayland-{self.name}",
            "KDE_SESSION_VERSION": "6",
            "XDG_CURRENT_DESKTOP": "KDE",
            "XDG_SESSION_TYPE": "wayland",
            "QT_QPA_PLATFORM": "wayland",
            "GDK_BACKEND": "wayland",
            # the user's, not those of root's shell, which a client would search and be refused
            "HOME": f"/home/{self.user}",
            "XDG_DATA_DIRS": f"{profile}/share:/run/current-system/sw/share",
            "XDG_CONFIG_DIRS": f"/etc/xdg:{profile}/etc/xdg:/run/current-system/sw/etc/xdg",
        }
        if self._x_display is not None:
            pairs["DISPLAY"] = self._x_display
        return " ".join(f"{key}={shlex.quote(value)}" for key, value in pairs.items())

    def as_user(self, command: str) -> str:
        """Runs `command` as the test user, and fails the test unless it succeeds."""
        return self.machine.succeed(self._wrap(command, session=False))

    def run(self, command: str) -> tuple[int, str]:
        """Runs `command` inside the session and returns its status and output."""
        return self.machine.execute(self._wrap(command))

    def sh(self, command: str) -> str:
        """Runs `command` inside the session, and fails the test unless it succeeds."""
        return self.machine.succeed(self._wrap(command))

    def spawn(self, command: str, log: str = "/dev/null") -> None:
        """Starts `command` in the background inside the session, as a client of KWin.

        Its output goes to the file `log`, or nowhere.
        """
        self.sh(f"setsid nohup {command} > {shlex.quote(log)} 2>&1 < /dev/null &")

    def colored_window(self, title: str, color: str, timeout: float = 30) -> None:
        """Opens a window that is one color all over, titled `title`, and waits for KWin to have it.

        The window is a `foot` terminal with a background of `color` (`rrggbb`) that is not
        see-through, running `sleep 3000`: a test that wants to read its pixels from a screenshot
        wants one color and nothing else, and [`Kde.close_colored_windows`][] finds it by the
        command it runs, since NixOS wraps programs and their process names are not what they seem.
        """
        words = f"foot -T {shlex.quote(title)} -o colors-dark.background={color}"
        self.spawn(f"{words} -o colors-dark.alpha=1.0 sleep 3000")
        self.wait_for(lambda: self.wid(title) != "", timeout=timeout)

    def close_colored_windows(self) -> None:
        """Closes every window [`Kde.colored_window`][] opened, and waits until they are gone."""
        self.run("pkill -f 'sleep 3000'")
        self.wait_for(lambda: self.run("pgrep -f 'sleep 3000'")[0] != 0, timeout=10)

    def systemctl(self, *args: str) -> str:
        """Runs `systemctl --user` as the test user."""
        return self.as_user("systemctl --user " + " ".join(args))

    def stop(self) -> None:
        """Stops the session's KWin and its bus."""
        self.systemctl(
            "stop", f"kwin-virtual@{self.name}.service", f"session-bus@{self.name}.service"
        )

    def restart_kwin(self) -> None:
        """Stops the session's KWin and starts a new one, on the same bus, and waits for it.

        The bus outlives KWin, as it does when the compositor of a running session crashes or is
        replaced. The clients of the old KWin go with it.
        """
        self.systemctl("restart", f"kwin-virtual@{self.name}.service")
        if not self.wait_for(lambda: self.has_name("org.kde.KWin"), timeout=60):
            log = self.machine.execute(f"journalctl _UID={self.uid} --no-pager -n 40")[1]
            raise AssertionError(f"KWin did not come back:\n{log}")

    def start_komorebi(self, *args: str) -> None:
        """Starts the daemon in this session, with `args`, as the user unit `komorebi@NAME`.

        The unit runs the NixOS module's `execStart` with `args` after it, against this
        session's bus and KWin, and writes its log to a file that [`Kde.komorebi_log`][] reads,
        emptied on every start. `execStart` already has `--log-level`, from the node's
        `services.komorebi.logLevel` (debug), and `komorebi` refuses a second one.
        """
        words = shlex.quote(" ".join(args))
        self.as_user(f"echo KOMOREBI_ARGS={words} > {self.runtime}/komorebi-{self.name}.env")
        self.systemctl("start", f"komorebi@{self.name}.service")

    def kill_komorebi(self) -> None:
        """Kills the daemon with `SIGKILL`, as a crash does, and waits for it to be gone.

        Nothing of what the daemon had set up is put back: the user's `kwinrc`, the desktops it
        made and the windows it hid are as it left them.
        """
        self.systemctl("kill", "--signal=KILL", f"komorebi@{self.name}.service")
        if not self.wait_for(lambda: not self.komorebi_running()):
            raise AssertionError("the daemon is still running after SIGKILL")

    def stop_komorebi(self) -> None:
        """Stops the daemon with `SIGTERM`, as a logout does, and waits for it to exit."""
        self.systemctl("stop", f"komorebi@{self.name}.service")

    def spawn_komorebi(self, *args: str) -> None:
        """Starts the daemon inside this session, in the background, with `args`.

        This is for the session node's own Plasma session, with arguments of the test's own:
        the daemon is a client of the session's KWin and bus, as one started from a terminal
        there is. The module's unit runs a daemon there too, with its `execStart` as it is, so a
        suite that spawns its own sets `services.komorebi.enable = false` in its node. The log
        goes to the file [`Kde.komorebi_log`][] reads, emptied on every start.
        """
        log = f"{self.runtime}/komorebi-{self.name}.log"
        self.spawn(shlex.join(["env", "NO_COLOR=1", "komorebi", *args]), log=log)

    def kill_spawned_komorebi(self) -> None:
        """Kills every daemon of the machine with `SIGKILL`, as a crash would, and waits for them
        to be gone: the one [`Kde.spawn_komorebi`][] started, which is not a unit to kill."""
        self.machine.execute("pkill -KILL -x komorebi")
        if not self.wait_for(lambda: self.machine.execute("pgrep -x komorebi")[0] != 0):
            raise AssertionError("komorebi is still running after SIGKILL")

    def shortcut_actions(self, component: str = "komorebi") -> list[str]:
        """The names of the actions `component` has in KDE's global shortcut daemon.

        That is the empty list when the component has none, or there is no such component.
        """
        status, out = self.run(
            "busctl --user --json=short call org.kde.kglobalaccel /kglobalaccel"
            f" org.kde.KGlobalAccel allActionsForComponent as 4 {shlex.quote(component)} '' '' ''"
        )
        if status != 0:
            return []
        return sorted(action[1] for action in json.loads(out)["data"][0])

    def komorebi_running(self) -> bool:
        """Returns true if and only if the daemon of this session is running."""
        unit = f"komorebi@{self.name}.service"
        command = self._wrap(f"systemctl --user is-active --quiet {unit}", session=False)
        return self.machine.execute(command)[0] == 0

    def komorebi_log(self) -> str:
        """The daemon's log since it was last started, or the empty string before that."""
        return self.machine.execute(f"cat {self.runtime}/komorebi-{self.name}.log")[1]

    def komorebic(self, *args: str) -> str:
        """Runs `komorebic` with `args` inside the session and returns what it printed.

        It fails the test unless `komorebic` succeeds, as it does when no daemon answers.
        `komorebic` finds the daemon's socket in the user's runtime directory, which every session
        of the user shares, so one daemon at a time can answer it.
        """
        return self.sh(shlex.join(["komorebic", *args]))

    def state(self, attempts: int = 10) -> dict[str, Any]:
        """The daemon's state, as `komorebic state` prints it.

        The daemon answers a query when it can take hold of the window manager within a second,
        and answers nothing when it cannot, which is what a daemon busy with a window that has
        just opened does on a machine that is loaded or slow: `komorebic` then prints an empty
        line and succeeds. It also gives the daemon a second to answer, and fails when that is not
        enough. The query is asked again, a second (times `scale`) after an empty answer or a
        failure, up to `attempts` times, and fails the test if the daemon does not answer every
        time. An answer that is not JSON and not empty fails at once.
        """
        for attempt in range(attempts):
            status, out = self.run(shlex.join(["komorebic", "state"]))
            if status == 0 and out.strip():
                return json.loads(out)
            if attempt + 1 < attempts:
                time.sleep(self.scale)
        raise AssertionError(
            f"the daemon did not answer `komorebic state`, {attempts} times: {status} {out!r}"
        )

    def focused_title(self, workspace: int = 0) -> str:
        """The title of the window the daemon has focused on a workspace of the first monitor.

        That is the focused window of the workspace's focused container, as the daemon's state
        has it, or the empty string when the workspace has no container.
        """
        containers = self._workspace(workspace)["containers"]
        if not containers["elements"]:
            return ""
        windows = containers["elements"][containers["focused"]]["windows"]
        return windows["elements"][windows["focused"]]["details"]["title"]

    def containers(self, workspace: int = 0) -> list[list[str]]:
        """The titles of the windows of each container of a workspace of the first monitor.

        The containers are in the order of the layout, and the windows of a stack in the order of
        the stack. A workspace with no container gives an empty list.
        """
        elements = self._workspace(workspace)["containers"]["elements"]
        return [
            [window["details"]["title"] for window in container["windows"]["elements"]]
            for container in elements
        ]

    def monocle(self, workspace: int = 0) -> list[str]:
        """The titles of the windows of the monocle container of a workspace of the first monitor.

        The windows are in the order of the stack. A workspace that is not in monocle gives an
        empty list.
        """
        container = self._workspace(workspace)["monocle_container"]
        if container is None:
            return []
        return [window["details"]["title"] for window in container["windows"]["elements"]]

    def managed(self, workspace: int = 0) -> int:
        """The number of windows the daemon has on a workspace of the first monitor."""
        containers = self._workspace(workspace)["containers"]["elements"]
        return sum(len(container["windows"]["elements"]) for container in containers)

    def managed_all(self) -> int:
        """The number of windows the daemon has, on every workspace of every monitor.

        That is the windows of the containers, the monocle container, the maximized window and
        the floating windows.
        """
        total = 0
        for monitor in self.state()["monitors"]["elements"]:
            for workspace in monitor["workspaces"]["elements"]:
                for container in workspace["containers"]["elements"]:
                    total += len(container["windows"]["elements"])
                if workspace["monocle_container"] is not None:
                    total += len(workspace["monocle_container"]["windows"]["elements"])
                if workspace["maximized_window"] is not None:
                    total += 1
                total += len(workspace["floating_windows"]["elements"])
        return total

    def monitor_bridge(self, path: str) -> None:
        """Starts recording the bridge's traffic on the session bus to `path`, in the background.

        `dbus-monitor` records every call to the daemon's `org.komorebi.Bridge` interface and every
        reply on the bus, which [`latency.rounds`](komorebi_vm.latency.rounds) reads.
        """
        rules = "\"type='method_call',interface='org.komorebi.Bridge'\" \"type='method_return'\""
        self.sh(f"setsid nohup dbus-monitor --session {rules} > {path} 2>&1 < /dev/null &")

    def script_loaded(self, name: str = "komorebi-bridge") -> bool:
        """Returns true if and only if KWin runs a script loaded under `name`."""
        status, out = self.run(
            "busctl --user call org.kde.KWin /Scripting org.kde.kwin.Scripting"
            f" isScriptLoaded s {shlex.quote(name)}"
        )
        return status == 0 and out.strip() == "b true"

    def wait_for(
        self,
        predicate: Callable[[], bool],
        timeout: float = 10,
        interval: float = 0.1,
    ) -> bool:
        """Polls `predicate` until it holds or `timeout` seconds (times `scale`) pass."""
        deadline = time.monotonic() + timeout * self.scale
        while True:
            if predicate():
                return True
            if time.monotonic() >= deadline:
                return False
            time.sleep(interval)

    def has_name(self, name: str) -> bool:
        """Returns true if and only if `name` is owned on the session bus."""
        status, _ = self.run(f"busctl --user --no-pager list | grep -q '^{name} '")
        return status == 0

    def wid(self, title: str) -> str:
        """The id of the first window whose title is exactly `title`, or the empty string."""
        pattern = shlex.quote(f"^{title}$")
        return self.run(f"kdotool search --name {pattern} | head -1")[1].strip()

    def wid_by_class(self, app: str) -> str:
        """The id of the first window whose class is exactly `app`, or the empty string.

        The class is the Wayland app id (`org.kde.konsole`), or the X11 class of a client
        on XWayland (`XTerm`).
        """
        pattern = shlex.quote(f"^{app}$")
        return self.run(f"kdotool search --class {pattern} | head -1")[1].strip()

    def rect(self, wid: str) -> Rect:
        """The position and size KWin reports for the window, rounded to whole pixels.

        At a fractional scale the frame is not a whole number of pixels in the logical
        coordinates KWin reports.
        """
        out = self.sh(f"kdotool getwindowgeometry {shlex.quote(wid)}")
        position = size = None
        for line in out.splitlines():
            words = line.split()
            if words[:1] == ["Position:"]:
                position = words[1]
            elif words[:1] == ["Geometry:"]:
                size = words[1]
        if position is None or size is None:
            raise AssertionError(f"no geometry in {out!r}")
        x, y = (round(float(v)) for v in position.split(","))
        w, h = (round(float(v)) for v in size.split("x"))
        return Rect(x, y, w, h)

    def title(self, wid: str) -> str:
        """The title of the window."""
        return self.sh(f"kdotool getwindowname {shlex.quote(wid)}").strip().splitlines()[-1]

    def active(self) -> str:
        """The id of the window KWin has focused, or the empty string."""
        lines = self.run("kdotool getactivewindow")[1].strip().splitlines()
        return lines[-1] if lines else ""

    def activate(self, wid: str) -> None:
        """Asks KWin to give the window the focus, as a click on it would."""
        self.sh(f"kdotool windowactivate {shlex.quote(wid)}")

    def minimize(self, wid: str) -> None:
        """Asks KWin to minimize the window, as its title bar button would."""
        self.sh(f"kdotool windowminimize {shlex.quote(wid)}")

    def maximize(self, wid: str) -> None:
        """Asks KWin to maximize the window both ways, as its title bar button would.

        It is the user's maximize: the daemon is not asked, and learns of it from KWin.
        """
        for state in ("MAXIMIZED_VERT", "MAXIMIZED_HORZ"):
            self.sh(f"kdotool windowstate --add {state} {shlex.quote(wid)}")

    def unmaximize(self, wid: str) -> None:
        """Asks KWin to restore the window from being maximized, as its title bar button would."""
        for state in ("MAXIMIZED_VERT", "MAXIMIZED_HORZ"):
            self.sh(f"kdotool windowstate --remove {state} {shlex.quote(wid)}")

    def fullscreen(self, wid: str, on: bool = True) -> None:
        """Asks KWin to take the window fullscreen, or with `on` false to bring it back from it.

        It is the user's fullscreen, as the program's own key does it: the daemon is not asked.
        """
        flag = "--add" if on else "--remove"
        self.sh(f"kdotool windowstate {flag} FULLSCREEN {shlex.quote(wid)}")

    def close(self, wid: str) -> None:
        """Asks KWin to close the window, as its title bar button would."""
        self.sh(f"kdotool windowclose {shlex.quote(wid)}")

    def shot(self, name: str) -> Image.Image:
        """Takes a screenshot of the machine's display and returns it, as RGB.

        The file is also kept as `NAME.png` in the test's output directory.
        """
        self.machine.screenshot(name)
        return Image.open(self.machine.out_dir / f"{name}.png").convert("RGB")

    def desk(self, wid: str) -> int:
        """The number of the virtual desktop the window is on, counted from 1 as kdotool counts.

        0 when kdotool cannot say (a window on every desktop, a window that is gone).
        """
        lines = self.run(f"kdotool get_desktop_for_window {shlex.quote(wid)}")[1].split()
        return int(lines[-1]) if lines and lines[-1].isdigit() else 0

    def shown(self) -> int:
        """The number of the virtual desktop KWin shows, counted from 1.

        With per-output virtual desktops, that is the desktop of the output KWin considers
        active; a suite with one output has no other.
        """
        lines = self.run("kdotool get_desktop")[1].split()
        return int(lines[-1]) if lines and lines[-1].isdigit() else 0

    def desktops(self) -> int:
        """The number of virtual desktops KWin has."""
        out = self.run(
            "busctl --user get-property org.kde.KWin /VirtualDesktopManager"
            " org.kde.KWin.VirtualDesktopManager count"
        )[1].split()
        return int(out[-1]) if out and out[-1].isdigit() else 0

    def desktop_names(self) -> list[str]:
        """The names of the virtual desktops KWin has, in order.

        The desktop at index `i` is the one `kdotool` calls `i + 1`.
        """
        out = self.run(
            "busctl --user get-property org.kde.KWin /VirtualDesktopManager"
            " org.kde.KWin.VirtualDesktopManager desktops"
        )[1]
        # `a(uss) 2 0 "id" "Desktop 1" 1 "id" "Desktop 2"`: the signature, how many, then a
        # position, an id and a name for each
        words = shlex.split(out)
        return words[4::3]

    def kwinrc(self, group: str, key: str) -> str | None:
        """The value of `key` in `[group]` of the user's `kwinrc`, or None when it is not set."""
        status, text = self.machine.execute(f"cat /home/{self.user}/.config/kwinrc")
        if status != 0:
            return None
        current = None
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("[") and line.endswith("]"):
                current = line[1:-1]
            elif current == group and "=" in line:
                name, value = line.split("=", 1)
                if name.strip() == key:
                    return value.strip()
        return None

    def kwinrc_group(self, group: str) -> dict[str, str]:
        """Every key of `[group]` of the user's `kwinrc` with its value, empty when there is none.

        A suite that wants to know that komorebi put the user's settings back compares this before
        and after.
        """
        status, text = self.machine.execute(f"cat /home/{self.user}/.config/kwinrc")
        keys: dict[str, str] = {}
        if status != 0:
            return keys
        current = None
        for line in text.splitlines():
            line = line.strip()
            if line.startswith("[") and line.endswith("]"):
                current = line[1:-1]
            elif current == group and "=" in line:
                name, value = line.split("=", 1)
                keys[name.strip()] = value.strip()
        return keys

    def effect_supported(self, name: str) -> bool:
        """Returns true if and only if KWin found the effect installed when it started."""
        return self._effects("isEffectSupported", name)

    def effect_loaded(self, name: str) -> bool:
        """Returns true if and only if KWin has the effect loaded."""
        return self._effects("isEffectLoaded", name)

    def unload_effect(self, name: str) -> None:
        """Asks KWin to unload the effect, whoever loaded it."""
        self.sh(
            "busctl --user call org.kde.KWin /Effects org.kde.kwin.Effects unloadEffect s"
            f" {shlex.quote(name)}"
        )

    def kwin_script(self, name: str, source: str, *, keep: bool = False) -> list[str]:
        """Runs `source` as the KWin script `name` and returns the lines it reported.

        The script reports with `report(...)`, which writes its arguments to KWin's log as one
        line marked with `name`, and ends with `report("done")`, which the lines are waited for.
        The script is unloaded afterwards, unless `keep` is set: a script that connects to a
        signal goes on reporting, and [`Kde.kwin_reports`][] reads what it reported since.
        """
        path = f"/tmp/kwin-script-{name}.js"
        prelude = (
            "function report() { print("
            + json.dumps(f"KS:{name} ")
            + " + Array.prototype.slice.call(arguments).join(' ')); }\n"
        )
        self.as_user(f"printf %s {shlex.quote(prelude + source)} > {path}")
        reply = self.sh(
            "busctl --user call org.kde.KWin /Scripting org.kde.kwin.Scripting loadScript ss"
            f" {path} {shlex.quote(name)}"
        ).split()
        script = f"/Scripting/Script{reply[-1]}"
        self.sh(f"busctl --user call org.kde.KWin {script} org.kde.kwin.Script run")
        if not self.wait_for(lambda: "done" in self.kwin_reports(name)):
            raise AssertionError(f"the KWin script {name} never reported it was done")
        if not keep:
            self.sh(
                "busctl --user call org.kde.KWin /Scripting org.kde.kwin.Scripting unloadScript s"
                f" {shlex.quote(name)}"
            )
        return [line for line in self.kwin_reports(name) if line != "done"]

    def work_area(self) -> Rect | None:
        """The area of the first output that windows may take, as KWin has it (its placement
        area: the output less the panels), or `None` when KWin does not say.

        The first output is the leftmost one. It is the area of the desktop the output shows, as
        the daemon's bridge reads it. A KWin script asks, because `kdotool` has no call for it.
        """
        source = """
        var first = null;
        for (var i = 0; i < workspace.screens.length; i++) {
            var s = workspace.screens[i];
            if (first === null || s.geometry.x < first.geometry.x
                || (s.geometry.x === first.geometry.x && s.geometry.y < first.geometry.y)) {
                first = s;
            }
        }
        if (first !== null) {
            var a = workspace.clientArea(
                KWin.PlacementArea, first, workspace.currentDesktopForScreen(first));
            report("area", Math.round(a.x), Math.round(a.y), Math.round(a.width),
                Math.round(a.height));
        }
        report("done");
        """
        try:
            lines = self.kwin_script(f"work-area-{next(_PROBES)}", source)
        except AssertionError:
            return None
        for line in lines:
            words = line.split()
            if words[:1] == ["area"] and len(words) == 5:
                try:
                    x, y, w, h = (int(word) for word in words[1:])
                except ValueError:
                    return None
                return Rect(x, y, w, h) if w > 0 and h > 0 else None
        return None

    def kwin_reports(self, name: str) -> list[str]:
        """Every line the KWin script `name` has reported so far ([`Kde.kwin_script`][])."""
        log = self.machine.execute(f"journalctl _UID={self.uid} --no-pager -o cat")[1]
        mark = f"KS:{name} "
        return [line.split(mark, 1)[1] for line in log.splitlines() if mark in line]

    @property
    def outputs(self) -> Outputs:
        """The session's outputs: `kde.outputs()` is how many are enabled, and `kde.outputs` has
        the ones to enable, disable and scale ([`Outputs`][])."""
        return Outputs(self)

    def _effects(self, method: str, name: str) -> bool:
        """Asks KWin's `/Effects` object a yes or no question about the effect `name`."""
        status, out = self.run(
            f"busctl --user call org.kde.KWin /Effects org.kde.kwin.Effects {method} s"
            f" {shlex.quote(name)}"
        )
        return status == 0 and out.strip() == "b true"

    def _workspace(self, workspace: int) -> dict[str, Any]:
        return self.state()["monitors"]["elements"][0]["workspaces"]["elements"][workspace]

    def _wrap(self, command: str, session: bool = True) -> str:
        env = self.env if session else f"XDG_RUNTIME_DIR={shlex.quote(self.runtime)}"
        return f"runuser -u {self.user} -- env {env} sh -c {shlex.quote(command)}"
