# Wallpapers, and the borders' colors that come from them, in a real Plasma session: what the shell
# shows and what is on the screen, read from the shell's D-Bus API and from screenshots.
#
#   A  a monitor's wallpaper is set at start, and a workspace's own replaces it when it is shown
#   B  a switch komorebi did not make (a pager's) sets the wallpaper too
#   C  the borders take the colors of the wallpaper's palette, and follow it from workspace to
#      workspace
#   D  a wallpaper the user changed by hand is put back when its workspace is shown again
#   E  the palette cache, and a path with a space in it (the second picture's)
#   F  komorebi that starts before the Plasma shell does
#
# The pictures are a dozen bands of color each, since a palette wants at least eight: a warm one for
# the monitor, a cool one for the second workspace, and a green one the test sets by hand.
import base64
import io
import re
import shlex
import time

from PIL import Image

from komorebi_vm import Checks, Kde, Rect, pixels

UNIT = "komorebi.service"
SHELL_UNIT = "plasma-plasmashell.service"
# the daemon's log of the day, without colors, which every run of the unit adds to
LOG = "/home/komorebi/.local/share/komorebi/komorebi_plaintext.log.*"
CONFIG = "/home/komorebi/.config/komorebi/komorebi.json"
PICTURES = "/tmp/wallpapers"
WARM = f"{PICTURES}/warm.png"
COOL = f"{PICTURES}/my pictures/cool.png"
OTHER = f"{PICTURES}/other.png"
WARM_URL = f"file://{PICTURES}/warm.png"
COOL_URL = f"file://{PICTURES}/my%20pictures/cool.png"
# `Image.PreserveAspectCrop`, which is how komorebi has the shell fill the screen
FILL = 2

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def picture(hue: tuple[int, int, int], ramp: int) -> bytes:
    """A 192 by 108 PNG of diagonal bands of `hue`, going dark and light, `ramp` varying the hue."""
    data: list[tuple[int, int, int]] = []
    for y in range(108):
        for x in range(192):
            band = (x // 16 + y // 16) % 12
            shade = 40 + (band % 4) * 55
            red, green, blue = hue
            green = min(255, green + band * ramp)
            data.append(
                (min(255, red * shade // 255), min(255, green * shade // 255), blue * shade // 255)
            )
    image = Image.new("RGB", (192, 108))
    image.putdata(data)
    out = io.BytesIO()
    image.save(out, "PNG")
    return out.getvalue()


def put(path: str, data: bytes) -> None:
    """Writes `data` to `path` in the machine, where the test user can read it."""
    directory = shlex.quote(path.rsplit("/", 1)[0])
    encoded = base64.b64encode(data).decode()
    machine.succeed(f"mkdir -p {directory} && chmod a+rx {directory}")
    machine.succeed(f"echo {encoded} | base64 -d > {shlex.quote(path)}")
    machine.succeed(f"chmod a+r {shlex.quote(path)}")


def running() -> bool:
    return kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def start_komorebi() -> None:
    kde.systemctl("start", UNIT)
    kde.wait_for(answering, timeout=60)
    time.sleep(2)


def stop_komorebi() -> None:
    kde.systemctl("stop", UNIT)
    kde.close_colored_windows()


def shell_wallpaper() -> tuple[str, int]:
    """What the shell shows on its first screen: the picture's URL, and the fill mode."""
    out = kde.run(
        "busctl --user call org.kde.plasmashell /PlasmaShell org.kde.PlasmaShell wallpaper u 0"
    )[1]
    image = re.search(r'"Image" s "([^"]*)"', out)
    fill = re.search(r'"FillMode" i (\d+)', out)
    return (image.group(1) if image else "", int(fill.group(1)) if fill else -1)


def shows(url: str) -> bool:
    return shell_wallpaper()[0] == url


def hue_of(image: Image.Image) -> str:
    """Warm, cool or neither: the average color of a grid of points of the screen."""
    width, height = image.size
    points = [
        pixels.pixel(image, width * i // 10, height * j // 12)
        for i in range(1, 10)
        for j in range(1, 9)
    ]
    red = sum(p[0] for p in points) / len(points)
    blue = sum(p[2] for p in points) / len(points)
    return "warm" if red > blue + 40 else "cool" if blue > red + 40 else "neither"


def screen_is(hue: str, name: str) -> bool:
    """Takes pictures of the screen until it is `hue`: one can catch the shell half way."""
    return kde.wait_for(lambda: hue_of(kde.shot(name)) == hue, timeout=20)


def ring_hue(frame: Rect, image: Image.Image) -> str:
    """The hue of the ring just outside the left edge of a frame."""
    red, _, blue = pixels.pixel(image, frame.x - 5, frame.y + frame.h // 2)
    return "warm" if red > blue + 40 else "cool" if blue > red + 40 else "neither"


def has_ring(title: str, hue: str, name: str) -> bool:
    """Returns true if the window has a ring of `hue` in a screenshot taken until it does."""
    return kde.wait_for(
        lambda: ring_hue(kde.rect(kde.wid(title)), kde.shot(name)) == hue, timeout=20
    )


print("== the pictures, and the wallpapers of the configuration")
put(WARM, picture((255, 0, 20), 20))
put(COOL, picture((20, 80, 255), 14))
put(OTHER, picture((60, 255, 60), 0))
kde.sh(f"mkdir -p {shlex.quote(CONFIG.rsplit('/', 1)[0])}")
config = (
    '{ "monitors": [ { "wallpaper": { "path": "' + WARM + '" },'
    ' "workspaces": [ { "name": "one" },'
    ' { "name": "two", "wallpaper": { "path": "' + COOL + '" } } ] } ] }'
)
kde.sh(f"echo {shlex.quote(config)} > {CONFIG}")
kde.sh("rm -rf ~/.cache/komorebi")

# the session started the module's unit when it logged in: this test starts its own runs
stop_komorebi()
kde.sh(f"plasma-apply-wallpaperimage {shlex.quote(OTHER)}")
checks.check(
    "the shell shows a picture of the user's choice to begin with",
    kde.wait_for(lambda: shows(f"file://{OTHER}"), timeout=20),
)

print("== A. set at start, and by workspace")
start_komorebi()
checks.check(
    "at start the shell shows the monitor's wallpaper",
    kde.wait_for(lambda: shows(WARM_URL), timeout=20),
)
checks.check("it fills the screen", shell_wallpaper()[1] == FILL)
checks.check("and the screen is warm", screen_is("warm", "a1"))

began = time.monotonic()
kde.komorebic("focus-workspace", "1")
checks.check(
    "workspace two's own wallpaper is shown when it is (the path has a space in it)",
    kde.wait_for(lambda: shows(COOL_URL), timeout=10),
)
took = time.monotonic() - began
checks.check(f"it took under two seconds ({took:.1f} s)", took < 2 * max(1, SCALE))
checks.check("and the screen is cool", screen_is("cool", "a2"))

kde.komorebic("focus-workspace", "0")
checks.check(
    "back on workspace one, the monitor's wallpaper again",
    kde.wait_for(lambda: shows(WARM_URL), timeout=10),
)

print("== B. a switch somebody else made")
kde.sh("kdotool set_desktop 2")
checks.check(
    "the desktop was changed outside komorebi, and its wallpaper came with it",
    kde.wait_for(lambda: shows(COOL_URL), timeout=10),
)
kde.sh("kdotool set_desktop 1")
checks.check("and back", kde.wait_for(lambda: shows(WARM_URL), timeout=10))

print("== C. the borders follow the wallpaper")
kde.colored_window("left", "303030")
time.sleep(1.5)
checks.check(
    "on the warm workspace the focused window's ring is warm", has_ring("left", "warm", "c1")
)
kde.komorebic("focus-workspace", "1")
time.sleep(2)
kde.colored_window("right", "303030")
time.sleep(2)
checks.check("on the cool one it is cool", has_ring("right", "cool", "c2"))
kde.komorebic("focus-workspace", "0")
time.sleep(2.5)
checks.check("and warm again when the first is back", has_ring("left", "warm", "c3"))

print("== D. a wallpaper changed by hand is put back")
kde.sh(f"plasma-apply-wallpaperimage {shlex.quote(OTHER)}")
checks.check(
    "the user changed it",
    kde.wait_for(lambda: shows(f"file://{OTHER}"), timeout=10),
)
kde.komorebic("focus-workspace", "1")
time.sleep(1)
kde.komorebic("focus-workspace", "0")
checks.check(
    "showing the workspace again set it back",
    kde.wait_for(lambda: shows(WARM_URL), timeout=10),
)

print("== E. the palette is kept")
cache = kde.sh("ls ~/.cache/komorebi 2>/dev/null || true").split()
print(f"     the cache: {cache}")
checks.check(
    "a palette for each wallpaper is in the cache, named for the picture",
    any(n.startswith("warm-") for n in cache) and any(n.startswith("cool-") for n in cache),
)
daemon_log = machine.execute(f"cat {LOG}")[1]
checks.check(
    "komorebi did not complain",
    "failed to set wallpaper" not in daemon_log
    and "gave up setting the wallpaper" not in daemon_log,
)
stop_komorebi()

print("== F. komorebi starts before the Plasma shell")
kde.sh(f"plasma-apply-wallpaperimage {shlex.quote(OTHER)}")
kde.systemctl("stop", SHELL_UNIT)
checks.check(
    "the shell is gone", kde.wait_for(lambda: not kde.has_name("org.kde.plasmashell"), timeout=20)
)
start_komorebi()
time.sleep(3)
checks.check("no shell to answer: komorebi is still managing", answering())
kde.systemctl("start", SHELL_UNIT)
checks.check(
    "the shell is back", kde.wait_for(lambda: kde.has_name("org.kde.plasmashell"), timeout=60)
)
checks.check(
    "once the shell is up the wallpaper is set",
    kde.wait_for(lambda: shows(WARM_URL), timeout=60),
)
daemon_log = machine.execute(f"cat {LOG}")[1]
checks.check(
    "and komorebi did not give up and say so", "gave up setting the wallpaper" not in daemon_log
)
stop_komorebi()

checks.finish()
