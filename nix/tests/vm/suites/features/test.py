# What komorebi on Windows does itself and komorebi on KDE has the compositor do, in a real Plasma
# session whose machine has the effects installed (the module does that): what is on the screen,
# read from screenshots, not what komorebi wrote. Each feature is a section, and a run of the
# daemon:
#
#   A  toggle-maximize: the window fills the work area, is the one on top, and goes back to its
#      tile. It runs on the daemon the session started with the module's unit, as on a user's
#      machine; the sections after it stop that daemon and start their own runs
#   C  border colors: every kind of window has its own: the focused window, the others, a locked
#      container that does not have the focus, a stack and a monocled window that have it, and a
#      floating window that has it
#
# Every window is a `foot` of one color all over (`Kde.colored_window`), and a color is read at the
# pixels just outside the frame of the window (`pixels.ring`). `komorebi/tests/maximize.rs` and
# `maximize_commands.rs` check what the window manager does with a maximize against the fake;
# section A checks that KWin's own maximize does it.
import time

from komorebi_vm import Checks, Kde, pixels

UNIT = "komorebi.service"
ROUNDED_CORNERS = "kwin4_effect_shapecorners"

BLUE = (0, 0, 255)
SINGLE = (66, 165, 245)
UNFOCUSED = (128, 128, 128)
LOCKED = (158, 8, 8)
STACK = (0, 165, 66)
MONOCLE = (255, 51, 153)
FLOATING = (245, 245, 165)

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def start_komorebi() -> None:
    kde.systemctl("start", UNIT)
    kde.wait_for(answering, timeout=60)
    # The shell's panel takes room from the windows a while after the shell has made it, and the
    # daemon learns that up to a second later. The windows are opened after that: before it they
    # are tiled under the panel and then again, and the ring is read in between.
    kde.wait_for_panel_room()
    # the effects are written, loaded and told to read their settings as the daemon starts
    time.sleep(2)


def stop_komorebi() -> None:
    kde.systemctl("stop", UNIT)
    kde.close_colored_windows()


def windows() -> None:
    """Opens a red, a green and a blue window, in that order, and waits for them to be tiled."""
    for title, color in (("red", "ff0000"), ("green", "00ff00"), ("blue", "0000ff")):
        kde.colored_window(title, color)
        time.sleep(1)
    kde.wait_for(lambda: kde.managed() == 3, timeout=30)
    time.sleep(2)


def ring_is(title: str, color: tuple[int, int, int]) -> bool:
    """Returns true if and only if the window with this title has a ring of `color` just outside
    its frame, in a screenshot taken now."""
    wid = kde.wid(title)
    return wid != "" and pixels.ring(kde.shot(f"ring-{title}"), kde.rect(wid), color) == []


def focus(title: str) -> None:
    """Gives the window the focus, and waits until komorebi has it focused too."""
    kde.activate(kde.wid(title))
    kde.wait_for(lambda: kde.focused_title() == title, timeout=10)


def near(pixel: object, want: tuple[int, int, int], tolerance: int = 14) -> bool:
    """True if every channel of the pixel is within `tolerance` of `want`."""
    assert isinstance(pixel, tuple)
    return all(abs(got - wanted) <= tolerance for got, wanted in zip(pixel, want, strict=True))


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

# a pixel that is not the middle of the window: komorebi puts the pointer there (mouse follows
# focus), and a screenshot has the white arrow on it
image = kde.shot("maximized")
print(f"     the screenshot is {image.size[0]}x{image.size[1]}")
inside = image.getpixel(
    (
        min(maximized.x + maximized.w // 4, image.width - 1),
        min(maximized.y + maximized.h // 4, image.height - 1),
    )
)
checks.check(f"and is the one on top ({inside})", near(inside, BLUE))
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


# the session started the module's unit when it logged in: section A used its daemon, and the
# sections after it start their own runs
stop_komorebi()

print("== C. border colors")
start_komorebi()
windows()
checks.check(
    "komorebi's build of the border effect is on the bus (it takes a color for each window)",
    kde.run(
        "busctl --user introspect org.kde.KWin /org/komorebi/BorderColours | grep -q SetColours"
    )[0]
    == 0,
)
checks.check(
    "the focused window has the focus color",
    kde.wait_for(lambda: ring_is("blue", SINGLE), timeout=15),
)
checks.check("the others have the unfocused one", ring_is("green", UNFOCUSED))

# a locked container that does not have the focus
focus("red")
kde.komorebic("toggle-lock")
focus("blue")
checks.check(
    "a locked container that does not have the focus is marked",
    kde.wait_for(lambda: ring_is("red", LOCKED), timeout=15),
)
focus("red")
kde.komorebic("toggle-lock")
focus("blue")

# a stack that has the focus: blue goes into the container on its left
kde.komorebic("stack", "left")
checks.check(
    "a stack with the focus has the stack color",
    kde.wait_for(lambda: ring_is("blue", STACK), timeout=15),
)
kde.komorebic("unstack")

# a monocled window
kde.komorebic("toggle-monocle")
checks.check(
    "a monocled window has the monocle color",
    kde.wait_for(lambda: ring_is("blue", MONOCLE), timeout=15),
)
kde.komorebic("toggle-monocle")

# a floating window that has the focus
kde.komorebic("toggle-float")
checks.check(
    "a floating window with the focus has the floating color",
    kde.wait_for(lambda: ring_is("blue", FLOATING), timeout=15),
)
stop_komorebi()

checks.finish()
