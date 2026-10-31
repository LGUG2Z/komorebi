"""Reading what a screenshot shows: the color of a pixel, and the ring around a window."""

from PIL import Image

from komorebi_vm.core.geometry import Rect

Color = tuple[int, int, int]


def pixel(image: Image.Image, x: int, y: int) -> Color:
    """The color of the pixel at `(x, y)`, as red, green and blue."""
    value = image.convert("RGB").getpixel((x, y))
    assert isinstance(value, tuple)
    return (int(value[0]), int(value[1]), int(value[2]))


def near(a: Color, b: Color, tolerance: int = 12) -> bool:
    """Returns true if and only if the two colors differ by at most `tolerance` on every channel.

    A screenshot is of a compositor that blends the shadow and the edge of a border into what is
    behind it, so a color is never asked for exactly.
    """
    return all(abs(x - y) <= tolerance for x, y in zip(a, b, strict=True))


def leftmost(image: Image.Image, y: int, color: Color, tolerance: int = 12) -> int | None:
    """Returns the `x` of the leftmost pixel of row `y` that is near `color`, or `None`.

    A window that moves across the screen has its left edge where this finds the first pixel of its
    color, which is how a test sees how far along it is.
    """
    for x in range(image.width):
        if near(pixel(image, x, y), color, tolerance):
            return x
    return None


def ring(image: Image.Image, frame: Rect, color: Color, margin: int = 5) -> list[str]:
    """Returns what is wrong with the ring around a window: a line for each side that has none.

    The ring is looked for `margin` pixels outside the middle of each of the four edges of
    `frame`, which is the window with its title bar, as the platform reports it. A side where that
    pixel is not near `color` is named, with the pixel it has. An empty list is a ring all round.
    """
    middle_x = frame.x + frame.w // 2
    middle_y = frame.y + frame.h // 2
    sides = {
        "above": (middle_x, frame.y - margin),
        "below": (middle_x, frame.y + frame.h + margin),
        "left": (frame.x - margin, middle_y),
        "right": (frame.x + frame.w + margin, middle_y),
    }
    wrong = []
    for name, (x, y) in sides.items():
        found = pixel(image, x, y)
        if not near(found, color):
            wrong.append(f"{name}: ({x}, {y}) is {found}, not {color}")
    return wrong
