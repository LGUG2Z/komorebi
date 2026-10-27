# The border and the animation, in a real Plasma session whose machine has the effects installed
# (the module does that): komorebi writes the effects' settings, loads them, and on exit puts
# everything back.
#
# What is checked is what is on the screen, not what komorebi wrote: a screenshot is read at the
# pixels just outside each window's frame (the border) and at the place a window is going to (the
# animation). The runs of komorebi:
#
#   A  the borders, as they are by default: the ring is outside the frame on all four sides
#   B  the commands that change them: the effect follows, and takes the border away and back
#   C  a 30 s animation: a window flipped to the other side is not there yet moments later
#   D  no animation, the control: the same flip is there at once
#
# Every window is a `foot` of one color all over (`Kde.colored_window`).
import time

from komorebi_vm import Checks, Kde, Rect, daemon, pixels

UNIT = "komorebi.service"
# the daemon's log of the day, without colors, which every run of the unit adds to
LOG = "/home/komorebi/.local/share/komorebi/komorebi_plaintext.log.*"
ROUNDED_CORNERS = "kwin4_effect_shapecorners"
GROUP = "Round-Corners"
GEOMETRY_CHANGE = "kwin4_effect_geometry_change"
ANIMATION_GROUP = "Effect-kwin4_effect_geometry_change"
FOCUSED = (66, 165, 245)
UNFOCUSED = (128, 128, 128)
RED = (255, 0, 0)

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def running() -> bool:
    return kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def asked(title: str) -> Rect | None:
    """The frame the daemon last asked KWin for, for the window with this title."""
    return daemon.asked(machine.execute(f"cat {LOG}")[1], title)


def start_komorebi() -> None:
    kde.systemctl("start", UNIT)
    kde.wait_for(answering, timeout=60)
    # The shell's panel takes room from the windows a while after the shell has made it, and the
    # daemon learns that up to a second later. The windows are opened after that: before it they
    # are tiled under the panel and then again, and the ring is read in between.
    kde.wait_for_panel_room()
    # the border effect is written, loaded and told to read its settings as the daemon starts
    time.sleep(2)


def stop_komorebi() -> None:
    kde.systemctl("stop", UNIT)
    kde.close_colored_windows()


print("== the effect is installed, and not loaded by anyone")
# the session started the module's unit when it logged in: this test starts its own runs
stop_komorebi()
checks.check("KDE Rounded Corners is installed", kde.effect_supported(ROUNDED_CORNERS))
checks.check("Geometry Change is installed", kde.effect_supported(GEOMETRY_CHANGE))
checks.check(
    "and KWin does not load either by itself (they are built disabled by default)",
    not kde.effect_loaded(ROUNDED_CORNERS) and not kde.effect_loaded(GEOMETRY_CHANGE),
)
before = kde.kwinrc_group(GROUP)
before_animation = kde.kwinrc_group(ANIMATION_GROUP)


def put_back() -> None:
    checks.check("komorebi exited", not running())
    checks.check("the border effect is unloaded again", not kde.effect_loaded(ROUNDED_CORNERS))
    checks.check("the animation effect is unloaded again", not kde.effect_loaded(GEOMETRY_CHANGE))
    checks.check(f"kwinrc [{GROUP}] is as it was", kde.kwinrc_group(GROUP) == before)
    checks.check(
        f"kwinrc [{ANIMATION_GROUP}] is as it was",
        kde.kwinrc_group(ANIMATION_GROUP) == before_animation,
    )


print("== A. komorebi starts, with the borders it has by default")
start_komorebi()
checks.check("the border effect is loaded", kde.effect_loaded(ROUNDED_CORNERS))
rc = kde.kwinrc_group(GROUP)
checks.check(
    "the border is the outer outline: 10 px, colored like the focused window",
    rc.get("OuterOutlineThickness") == "10" and rc.get("OuterOutlineColor") == "66,165,245",
)
checks.check(
    "nothing is drawn inside the window, over its title bar (the effect's default does)",
    rc.get("OutlineThickness") == "0" and rc.get("SecondOutlineThickness") == "0",
)
checks.check(
    "the unfocused window gets the other color",
    rc.get("InactiveOuterOutlineColor") == "128,128,128",
)
checks.check("the corner radius is the border radius", rc.get("Size") == "10")

print("== A. two windows, side by side")
kde.colored_window("red", "ff0000")
time.sleep(1)
kde.colored_window("blue", "0000ff")
checks.check("komorebi tiles both", kde.wait_for(lambda: kde.managed() == 2, timeout=30))
time.sleep(3)
red, blue = kde.wid("red"), kde.wid("blue")
red_frame, blue_frame = kde.rect(red), kde.rect(blue)
print(f"     red: {red_frame}   blue: {blue_frame}")
checks.check("they are next to each other", not red_frame.overlaps(blue_frame))
image = kde.shot("tiled")

print("== A. the border is just outside the frame, on all four sides")
wrong = pixels.ring(image, blue_frame, FOCUSED)
checks.check(
    "the focused window (blue) has the focus color around it"
    + "".join(f"\n       {w}" for w in wrong),
    wrong == [],
)
wrong = pixels.ring(image, red_frame, UNFOCUSED)
checks.check(
    "the other window (red) has the unfocused color around it"
    + "".join(f"\n       {w}" for w in wrong),
    wrong == [],
)
inside = pixels.pixel(image, blue_frame.x + 40, blue_frame.y + 5)
checks.check(
    f"the border does not cover the title bar (inside the frame, near its top: {inside})",
    not pixels.near(inside, FOCUSED),
)

print("== B. the commands that set the border reach the effect")
kde.komorebic("border-width", "6")
checks.check(
    "a narrower border is a thinner outline",
    kde.wait_for(lambda: kde.kwinrc_group(GROUP).get("OuterOutlineThickness") == "6", timeout=10),
)
kde.komorebic("border", "disable")
checks.check(
    "no border unloads the effect", kde.wait_for(lambda: not kde.effect_loaded(ROUNDED_CORNERS))
)
checks.check(f"and puts kwinrc [{GROUP}] back as it was", kde.kwinrc_group(GROUP) == before)
kde.komorebic("border", "enable")
checks.check(
    "a border again loads the effect, with the width that was set",
    kde.wait_for(lambda: kde.effect_loaded(ROUNDED_CORNERS), timeout=10)
    and kde.kwinrc_group(GROUP).get("OuterOutlineThickness") == "6",
)

print("== B. the layout leaves the room the border takes")
room = asked("blue")
kde.komorebic("border-width", "0")
kde.komorebic("retile")


def grew() -> bool:
    now = asked("blue")
    return room is not None and now is not None and (now.w - room.w, now.h - room.h) == (20, 20)


checks.check(
    f"without the 10 px of border on each side the tile is 20 px wider and higher ({room})",
    kde.wait_for(grew, timeout=10),
)

stop_komorebi()
put_back()


def flip_of_red() -> tuple[Rect, Rect]:
    """Flips the layout, and returns the frame red had and the one komorebi gave it."""
    kde.colored_window("red", "ff0000")
    time.sleep(1)
    kde.colored_window("blue", "0000ff")
    kde.wait_for(lambda: kde.managed() == 2, timeout=30)
    # well past the half of it, where an ease out is at 97 %: the windows have got where they go
    time.sleep(16 * SCALE)
    before_flip = kde.rect(kde.wid("red"))
    kde.komorebic("flip-layout", "horizontal")
    kde.wait_for(lambda: kde.rect(kde.wid("red")) != before_flip, timeout=10)
    return before_flip, kde.rect(kde.wid("red"))


def point_in(frame: Rect) -> tuple[int, int]:
    """A point inside the frame, far from where red came from, which is on its left."""
    return frame.x + frame.w - 60, frame.y + frame.h // 2


print("== C. komorebi runs a 30 s animation")
start_komorebi()
kde.komorebic("animation-duration", "30000")
# an ease out is at 97 % after half of its time, which the waits below rely on (the default style
# is linear, which is at half)
kde.komorebic("animation-style", "--style", "ease-out-expo")
kde.komorebic("animation", "enable")
checks.check(
    "the animation effect is loaded",
    kde.wait_for(lambda: kde.effect_loaded(GEOMETRY_CHANGE), timeout=10),
)
animation = kde.kwinrc_group(ANIMATION_GROUP)
checks.check(
    "it lasts what it was asked to, without cross-fading",
    animation.get("Duration") == "30000" and animation.get("CrossFade") == "false",
)
was, now = flip_of_red()
print(f"     red: {was} -> {now}")
checks.check("komorebi moved the window", was != now)
x, y = point_in(now)
soon = pixels.pixel(kde.shot("soon"), x, y)
checks.check(
    f"a moment after the flip red has not got to its new place ({soon})",
    not pixels.near(soon, RED),
)
time.sleep(16 * SCALE)
later = pixels.pixel(kde.shot("later"), x, y)
checks.check(f"and 16 s later it has ({later})", pixels.near(later, RED))
stop_komorebi()
put_back()

print("== D. without an animation, the control, the same flip is there at once")
start_komorebi()
checks.check("the animation effect is not loaded", not kde.effect_loaded(GEOMETRY_CHANGE))
was, now = flip_of_red()
x, y = point_in(now)
at_once = pixels.pixel(kde.shot("control"), x, y)
checks.check(
    f"red is at its new place when the screenshot is taken ({at_once})",
    pixels.near(at_once, RED),
)
stop_komorebi()
put_back()

checks.finish()
