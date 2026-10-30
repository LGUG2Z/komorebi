"""Wallpapers, and the colors of the borders that come from them.

A monitor's wallpaper is set when the daemon starts, and the wallpaper of a workspace replaces it
while the workspace is shown. The borders of the windows take the colors of the wallpaper's
palette, and follow it from workspace to workspace. A wallpaper the user changed by hand is put back
when its workspace is shown again.

The pictures are a dozen bands of color each, since a palette wants at least eight: a warm one for
the monitor, a cool one for the second workspace, and a green one the user sets by hand. Both ports
read the same `wallpaper` of `komorebi.json`, and keep a palette for each picture.
"""

import io

from PIL import Image

from komorebi_vm.core import pixels
from komorebi_vm.core.checks import Checks
from komorebi_vm.core.geometry import Rect
from komorebi_vm.core.platform import Client, Platform


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


def _hue(red: int, blue: int) -> str:
    return "warm" if red > blue + 40 else "cool" if blue > red + 40 else "neither"


def hue_of(image: Image.Image) -> str:
    """Warm, cool or neither: the average color of a grid of points of the screen."""
    width, height = image.size
    points = [
        pixels.pixel(image, width * i // 10, height * j // 12)
        for i in range(1, 10)
        for j in range(1, 9)
    ]
    return _hue(
        round(sum(p[0] for p in points) / len(points)),
        round(sum(p[2] for p in points) / len(points)),
    )


def ring_hue(frame: Rect, image: Image.Image, reach: int = 12) -> str:
    """The hue of the ring around a window: the colors within `reach` pixels of the left edge of
    its frame, on either side of it, at its middle. The window's own color is neutral, so what has
    a hue is the border; where the ports draw it, outside the frame or on its edge, differs."""
    seen = set()
    for x in range(max(0, frame.x - reach), min(image.size[0], frame.x + reach)):
        red, _, blue = pixels.pixel(image, x, frame.y + frame.h // 2)
        seen.add(_hue(red, blue))
    seen.discard("neither")
    return seen.pop() if len(seen) == 1 else "neither" if not seen else "mixed"


def run(platform: Platform, checks: Checks) -> None:
    """The daemon starts with a wallpaper for the monitor and one for its second workspace; the
    scenario follows them from workspace to workspace, with the borders, and sets one by hand.
    The workspace is empty, and so it is when the scenario ends."""
    warm = platform.temp_path("wallpapers/warm.png")
    cool = platform.temp_path("wallpapers/my pictures/cool.png")
    other = platform.temp_path("wallpapers/other.png")
    platform.put_file(warm, picture((255, 0, 20), 20))
    platform.put_file(cool, picture((20, 80, 255), 14))
    platform.put_file(other, picture((60, 255, 60), 0))

    def shows(path: str) -> bool:
        return platform.wallpaper() == path

    def screen_is(hue: str, name: str) -> bool:
        """Takes pictures of the screen until it is `hue`: one can catch the shell half way."""
        return platform.wait_for(lambda: hue_of(platform.screenshot(name)) == hue, timeout=20)

    def has_ring(client: Client, hue: str, name: str) -> bool:
        """The window has a ring of `hue` in a screenshot taken until it does."""

        def ring() -> bool:
            window = platform.os_window(client)
            return window is not None and ring_hue(window.frame, platform.screenshot(name)) == hue

        return platform.wait_for(ring, timeout=20)

    platform.set_wallpaper(other)
    checks.check(
        "the screen shows a picture of the user's choice to begin with",
        platform.wait_for(lambda: shows(other), timeout=20),
    )

    platform.restart_with(
        {
            "monitors": [
                {
                    "wallpaper": {"path": warm},
                    "workspaces": [
                        {"name": "one"},
                        {"name": "two", "wallpaper": {"path": cool}},
                    ],
                }
            ]
        }
    )
    checks.check(
        "at start the screen shows the monitor's wallpaper",
        platform.wait_for(lambda: shows(warm), timeout=20),
    )
    checks.check("and the screen is warm", screen_is("warm", "wallpaper-a1"))

    platform.komorebic("focus-workspace", "1")
    checks.check(
        "workspace two's own wallpaper is shown when it is (the path has a space in it)",
        platform.wait_for(lambda: shows(cool), timeout=10),
    )
    checks.check("and the screen is cool", screen_is("cool", "wallpaper-a2"))
    platform.komorebic("focus-workspace", "0")
    checks.check(
        "back on workspace one, the monitor's wallpaper again",
        platform.wait_for(lambda: shows(warm), timeout=10),
    )

    left = platform.spawn("wallpaper-left")
    checks.check(
        "on the warm workspace the focused window's ring is warm",
        has_ring(left, "warm", "wallpaper-c1"),
    )
    platform.komorebic("focus-workspace", "1")
    right = platform.spawn("wallpaper-right")
    checks.check("on the cool one it is cool", has_ring(right, "cool", "wallpaper-c2"))
    platform.komorebic("focus-workspace", "0")
    checks.check("and warm again when the first is back", has_ring(left, "warm", "wallpaper-c3"))

    platform.set_wallpaper(other)
    checks.check("the user changed it", platform.wait_for(lambda: shows(other), timeout=10))
    platform.komorebic("focus-workspace", "1")
    platform.wait_for(lambda: shows(cool), timeout=10)
    platform.komorebic("focus-workspace", "0")
    checks.check(
        "showing the workspace again set it back",
        platform.wait_for(lambda: shows(warm), timeout=10),
    )

    platform.close(left)
    platform.komorebic("focus-workspace", "1")
    platform.close(right)
    platform.komorebic("focus-workspace", "0")
