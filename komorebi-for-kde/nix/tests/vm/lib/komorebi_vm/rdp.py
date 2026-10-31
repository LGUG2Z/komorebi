"""A FreeRDP client on a session node, for the suites that check KRDP.

KRDP, Plasma's RDP server, runs in the session as a user unit (`nix/dev/krdp.nix`). What a suite
can tell is whether a client with the password is let in and one without it is not, and whether
the picture the client gets is what the display shows. The client is FreeRDP's, on an X server of
its own (Xvfb), so that its picture can be read back and compared with a screenshot of the
display: a client on the display would be in its own picture. No keys are sent, since no client
could script them against KRDP (docs/dev/PLATFORM.md).
"""

import shlex

from PIL import Image, ImageChops, ImageStat

from komorebi_vm.kde import Kde
from komorebi_vm.machine import Machine


def difference(remote: Image.Image, local: Image.Image) -> float:
    """The mean difference of two pictures, from 0 (the same) to 255, over every channel.

    The first is resized to the size of the second when they differ.
    """
    if remote.size != local.size:
        remote = remote.resize(local.size)
    return sum(ImageStat.Stat(ImageChops.difference(remote, local)).mean) / 3


def colors(image: Image.Image) -> int:
    """The number of different colors a picture has."""
    found = image.getcolors(maxcolors=image.width * image.height)
    return len(found) if found is not None else 0


class RdpClient:
    """FreeRDP's client, logging in to the KRDP of a session node as its user.

    `size` is the size of the client's X server and of the display it gets a picture of.
    """

    def __init__(
        self,
        machine: Machine,
        kde: Kde,
        *,
        x_display: str = ":99",
        size: tuple[int, int] = (1280, 800),
        user: str = "komorebi",
    ) -> None:
        self.machine = machine
        self.kde = kde
        self.x_display = x_display
        self.size = size
        self.user = user

    def listening(self) -> bool:
        """Returns true if and only if KRDP's server has port 3389 in the guest.

        QEMU's port forwarding accepts a connection whether or not anything listens behind it,
        so this asks the guest, which `ss` sees.
        """
        return self.machine.execute("ss -ltnpH 'sport = :3389' | grep -q krdpserver")[0] == 0

    def start_x_server(self) -> None:
        """Starts the client's X server and waits for its socket, for up to 30 seconds."""
        width, height = self.size
        self.machine.succeed(
            f"setsid nohup Xvfb {self.x_display} -screen 0 {width}x{height}x24"
            " > /tmp/xvfb.log 2>&1 < /dev/null &"
        )
        socket = f"/tmp/.X11-unix/X{self.x_display[1:]}"
        self.machine.succeed(f"timeout 30 sh -c 'until test -S {socket}; do sleep 1; done'")

    def command(self, secret: str, *args: str) -> str:
        """The command line of the client on its X server, logging in with `secret` as the user."""
        words = [
            "xfreerdp",
            "/v:127.0.0.1",
            f"/u:{self.user}",
            f"/p:{secret}",
            "/cert:ignore",
            *args,
        ]
        return f"env DISPLAY={self.x_display} " + " ".join(shlex.quote(word) for word in words)

    def logs_in(self, secret: str) -> bool:
        """Returns true if and only if the server lets a client with `secret` in (`/auth-only`)."""
        return self.machine.execute(self.command(secret, "/auth-only"))[0] == 0

    def connect(self, secret: str) -> None:
        """Starts a client that stays connected, in the background, with a window of `size`."""
        width, height = self.size
        command = self.command(secret, f"/size:{width}x{height}")
        self.machine.succeed(f"setsid nohup {command} > /tmp/xfreerdp.log 2>&1 < /dev/null &")

    def picture(self) -> Image.Image:
        """What the client shows, read from its X server."""
        self.machine.succeed(
            f"xwd -display {self.x_display} -root -silent | xwdtopnm > /tmp/rdp.ppm 2>/dev/null"
        )
        self.machine.copy_from_machine("/tmp/rdp.ppm")
        return Image.open(self.machine.out_dir / "rdp.ppm").convert("RGB")

    def shows_display(self, timeout: float = 90) -> bool:
        """Waits for the client's picture to be the display, and says whether it was.

        A client that has no picture yet shows black, a desktop has thousands of colors, and the
        video KRDP sends is lossy, so the picture is the display when it has more than a thousand
        colors and is within 16 of 255, on average, of a screenshot taken right after it.
        """

        def same() -> bool:
            remote = self.picture()
            local = self.kde.shot("display")
            diff = difference(remote, local)
            print(
                f"the client's picture: {remote.size}, {colors(remote)} colors,"
                f" {diff:.1f} from the display"
            )
            return colors(remote) > 1000 and diff < 16

        return self.kde.wait_for(same, timeout=timeout, interval=2)
