# KRDP, Plasma's RDP server, runs in the Plasma session of the session node as the guest to develop
# in runs it (nix/dev/krdp.nix), and a client logs in and gets a picture of the desktop. What only a
# real session can tell: that the unit starts in the session once it has a password, that KWin lets
# KRDP have the screen without asking, that a client with the password is let in and one without
# it is not, and that the picture the client gets is what the display shows.
#
# The client is FreeRDP's, on an X server of its own (Xvfb), so its picture can be read back and
# compared with a screenshot of the display. A client on the display would be in its own picture.
# No keys are sent: no client could script them against KRDP (docs/dev/PLATFORM.md).
import secrets
import time

from PIL import Image, ImageChops, ImageStat

from komorebi_vm import Checks, Kde

PASSWORD_FILE = "/home/komorebi/.config/komorebi-vm/rdp-password"
# the client's X server, and the size of the display it gets a picture of
X = ":99"
WIDTH, HEIGHT = 1280, 800

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def active(unit: str) -> bool:
    return kde.run(f"systemctl --user is-active --quiet {unit}")[0] == 0


checks.check("komorebi runs in the session", kde.wait_for(lambda: active("komorebi"), timeout=60))
checks.check("with no password, KRDP does not start", not active("krdp"))

password = secrets.token_hex(16)
kde.as_user(
    f"umask 077 && mkdir -p $(dirname {PASSWORD_FILE}) && printf %s {password} > {PASSWORD_FILE}"
)
kde.systemctl("start", "krdp.service")


def listening() -> bool:
    return machine.execute("ss -ltnpH 'sport = :3389' | grep -q krdpserver")[0] == 0


checks.check("with a password, KRDP listens on port 3389", kde.wait_for(listening, timeout=60))

machine.succeed(
    f"setsid nohup Xvfb {X} -screen 0 {WIDTH}x{HEIGHT}x24 > /tmp/xvfb.log 2>&1 < /dev/null &"
)
machine.wait_until_succeeds(f"test -S /tmp/.X11-unix/X{X[1:]}", timeout=30)


def client(secret: str, *args: str) -> str:
    """The command line of FreeRDP's client, on the client's X server, logging in as komorebi."""
    words = ["xfreerdp", "/v:127.0.0.1", "/u:komorebi", f"/p:{secret}", "/cert:ignore", *args]
    return f"env DISPLAY={X} " + " ".join(words)


checks.check(
    "a client with the password is let in",
    machine.execute(client(password, "/auth-only"))[0] == 0,
)
checks.check(
    "a client with another password is not",
    machine.execute(client("not-" + password, "/auth-only"))[0] != 0,
)

started = time.monotonic()
machine.succeed(
    "setsid nohup "
    + client(password, f"/size:{WIDTH}x{HEIGHT}")
    + " > /tmp/xfreerdp.log 2>&1 < /dev/null &"
)


def client_picture() -> Image.Image:
    """What the client shows, read from its X server."""
    machine.succeed(f"xwd -display {X} -root -silent | xwdtopnm > /tmp/rdp.ppm 2>/dev/null")
    machine.copy_from_machine("/tmp/rdp.ppm")
    return Image.open(machine.out_dir / "rdp.ppm").convert("RGB")


def difference(remote: Image.Image, local: Image.Image) -> float:
    """The mean difference of the two pictures, from 0 (the same) to 255, over every channel."""
    if remote.size != local.size:
        remote = remote.resize(local.size)
    return sum(ImageStat.Stat(ImageChops.difference(remote, local)).mean) / 3


def colors(image: Image.Image) -> int:
    found = image.getcolors(maxcolors=image.width * image.height)
    return len(found) if found is not None else 0


def same_picture() -> bool:
    remote = client_picture()
    local = kde.shot("display")
    diff = difference(remote, local)
    print(f"the client's picture: {remote.size}, {colors(remote)} colors, {diff:.1f} from the display")
    # a client that has no picture yet shows black, a desktop has thousands of colors, and the video
    # KRDP sends is lossy
    return colors(remote) > 1000 and diff < 16


checks.check("the client's picture is the display", kde.wait_for(same_picture, timeout=90, interval=2))
print(f"after {time.monotonic() - started:.1f}s")

remote = client_picture()
remote.save(machine.out_dir / "client.png")
print(machine.execute("journalctl --no-pager _SYSTEMD_USER_UNIT=krdp.service | tail -n 20")[1])
print(machine.execute("tail -n 20 /tmp/xfreerdp.log")[1])

checks.finish()
