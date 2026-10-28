# What komorebi does that is on the screen of a real compositing session, read from screenshots: the
# session guest draws, and the daemon is the module's unit, as on a user's machine. A headless KWin
# cannot draw, so what the compositor puts on top can only be told here.
#
#   A. toggle-maximize   the window fills the work area, is the one on top, and goes back to its
#                        tile
#
# `komorebi/tests/maximize.rs` and `maximize_commands.rs` check what the window manager does against
# the fake; this checks that KWin's own maximize does it.
import time

from komorebi_vm import Checks, Kde

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)

BLUE = (0, 0, 255)


def near(pixel: object, want: tuple[int, int, int], tolerance: int = 14) -> bool:
    """True if every channel of the pixel is within `tolerance` of `want`."""
    assert isinstance(pixel, tuple)
    return all(abs(got - wanted) <= tolerance for got, wanted in zip(pixel, want, strict=True))


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def open_window(title: str, background: str) -> None:
    """Opens a terminal with a plain background, so that a screenshot says which window is on top."""
    kde.spawn(
        f"foot -T {title} -o colors-dark.background={background} -o colors-dark.alpha=1.0"
        " sleep 3000"
    )
    kde.wait_for(lambda: kde.wid(title) != "", timeout=60)


print("== A. toggle-maximize")
checks.check("the session started komorebi", kde.wait_for(answering, timeout=120))
for count, (title, background) in enumerate(
    [("red", "ff0000"), ("green", "00ff00"), ("blue", "0000ff")], start=1
):
    open_window(title, background)
    checks.check(
        f"komorebi manages {title}", kde.wait_for(lambda count=count: kde.managed() == count)
    )
# komorebi tiles the windows, and the last one to open has the focus. The bridge asks again for the
# position of a window that is not where komorebi put it for a second after it did (PLATFORM.md,
# "How clients answer a frame"), which would put a window that is maximized in that second back
# where its tile was: the test waits that second out, as a person would.
kde.wait_for(lambda: kde.focused_title() == "blue", timeout=20)
time.sleep(3 * SCALE)

area = kde.state()["monitors"]["elements"][0]["work_area_size"]
blue = kde.wid("blue")
tile = kde.rect(blue)
print(f"     the work area is {area['right']}x{area['bottom']}, blue's tile is {tile}")
checks.check(
    "blue is a tile, not the whole work area",
    tile.w < area["right"] - 20 or tile.h < area["bottom"] - 20,
)

kde.komorebic("toggle-maximize")


def fills_the_work_area() -> bool:
    """KWin's maximize leaves the panel alone, which is at most 100 pixels of the screen."""
    r = kde.rect(blue)
    return (
        r.x <= area["left"] + 2
        and r.y <= area["top"] + 2
        and r.w >= area["right"] - 30
        and r.h >= area["bottom"] - 100
    )


filled = kde.wait_for(fills_the_work_area, timeout=20)
maximized = kde.rect(blue)
checks.check(
    f"the focused window fills the work area (blue is {maximized}, the area is {area})", filled
)
image = kde.shot("maximized")
print(f"     the screenshot is {image.size[0]}x{image.size[1]}")
middle = image.getpixel(
    (
        min(maximized.x + maximized.w // 2, image.width - 1),
        min(maximized.y + maximized.h // 2, image.height - 1),
    )
)
checks.check(f"and is the one on top ({middle})", near(middle, BLUE))
workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
checks.check(
    "komorebi's state has it as the maximized window, out of its containers",
    workspace["maximized_window"] is not None and len(workspace["containers"]["elements"]) == 2,
)
checks.check("komorebi still has all three", kde.managed_all() == 3)

kde.komorebic("toggle-maximize")
checks.check(
    "toggled back, it is in its tile again",
    kde.wait_for(lambda: kde.rect(blue) == tile, timeout=20),
)
workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
checks.check(
    "... and in a container of its own",
    workspace["maximized_window"] is None and len(workspace["containers"]["elements"]) == 3,
)

checks.finish()
