# A Plasma session draws a desktop in the test machine, so that every test after this one can trust
# a screenshot. KSplash has thousands of colors on it too, so "not blank" is not the check: the
# splash has to be gone and a window has to be painted on the desktop.
import time

from komorebi_vm import Checks, Kde

start_all()
started = time.monotonic()
machine.wait_for_unit("multi-user.target")

checks = Checks()

kde = Kde.session(machine, scale=SCALE)
print(f"the desktop is up after {time.monotonic() - started:.0f}s")


def konsole() -> str:
    return kde.run("kdotool search --class konsole | head -1")[1].strip()


kde.spawn("konsole")
checks.check("a window appears", kde.wait_for(lambda: konsole() != "", timeout=60))
rect = kde.rect(konsole())
checks.check("the window has a size", rect.w > 0 and rect.h > 0)


def painted() -> bool:
    image = kde.shot("desktop")
    center = image.getpixel((rect.x + rect.w // 2, rect.y + rect.h // 2))
    assert isinstance(center, tuple)
    # the terminal's background is dark; the wallpaper behind it is not
    return sum(center) < 3 * 90


checks.check("the window is painted on the screen", kde.wait_for(painted, timeout=60))

image = kde.shot("desktop")
colors = image.getcolors(maxcolors=image.width * image.height)
assert colors is not None
print(f"the screenshot is {image.size[0]}x{image.size[1]} with {len(colors)} colors")
checks.check("the desktop behind it is drawn", len(colors) > 5000)

checks.finish()
