"""A KWin session inside a test machine, and the tools to look at it."""

import shlex
import time
from collections.abc import Callable
from dataclasses import dataclass

from PIL import Image

from komorebi_vm.machine import Machine


@dataclass(frozen=True)
class Rect:
    """A window's position and size, in pixels."""

    x: int
    y: int
    w: int
    h: int

    def overlaps(self, other: "Rect") -> bool:
        """Returns true if and only if the two rectangles share at least one pixel."""
        return (
            self.x < other.x + other.w
            and other.x < self.x + self.w
            and self.y < other.y + other.h
            and other.y < self.y + self.h
        )


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
        Wayland socket, and the splash screen has gone. The last condition is
        the one a screenshot test depends on: a splash screen already has
        thousands of colors on it. NixOS wraps its programs, so the processes
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
        return kde

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

    def systemctl(self, *args: str) -> str:
        """Runs `systemctl --user` as the test user."""
        return self.as_user("systemctl --user " + " ".join(args))

    def stop(self) -> None:
        """Stops the session's KWin and its bus."""
        self.systemctl(
            "stop", f"kwin-virtual@{self.name}.service", f"session-bus@{self.name}.service"
        )

    def start_komorebi(self, *args: str) -> None:
        """Starts the daemon in this session, with `args`, as the user unit `komorebi@NAME`.

        The unit runs the `komorebi` the node has, against this session's bus and KWin, and
        writes its log to a file that [`Kde.komorebi_log`][] reads, emptied on every start.
        """
        words = shlex.quote(" ".join(args))
        self.as_user(f"echo KOMOREBI_ARGS={words} > {self.runtime}/komorebi-{self.name}.env")
        self.systemctl("start", f"komorebi@{self.name}.service")

    def stop_komorebi(self) -> None:
        """Stops the daemon with `SIGTERM`, as a logout does, and waits for it to exit."""
        self.systemctl("stop", f"komorebi@{self.name}.service")

    def komorebi_running(self) -> bool:
        """Returns true if and only if the daemon of this session is running."""
        unit = f"komorebi@{self.name}.service"
        command = self._wrap(f"systemctl --user is-active --quiet {unit}", session=False)
        return self.machine.execute(command)[0] == 0

    def komorebi_log(self) -> str:
        """The daemon's log since it was last started, or the empty string before that."""
        return self.machine.execute(f"cat {self.runtime}/komorebi-{self.name}.log")[1]

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
        """The position and size KWin reports for the window."""
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
        x, y = (int(v) for v in position.split(","))
        w, h = (int(v) for v in size.split("x"))
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

    def close(self, wid: str) -> None:
        """Asks KWin to close the window, as its title bar button would."""
        self.sh(f"kdotool windowclose {shlex.quote(wid)}")

    def shot(self, name: str) -> Image.Image:
        """Takes a screenshot of the machine's display and returns it, as RGB.

        The file is also kept as `NAME.png` in the test's output directory.
        """
        self.machine.screenshot(name)
        return Image.open(self.machine.out_dir / f"{name}.png").convert("RGB")

    def outputs(self) -> int:
        """The number of outputs KWin has enabled."""
        return int(self.sh("kscreen-doctor -j | jq '[.outputs[] | select(.enabled)] | length'"))

    def _wrap(self, command: str, session: bool = True) -> str:
        env = self.env if session else f"XDG_RUNTIME_DIR={shlex.quote(self.runtime)}"
        return f"runuser -u {self.user} -- env {env} sh -c {shlex.quote(command)}"
