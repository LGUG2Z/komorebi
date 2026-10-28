# Focus follows mouse against a real KWin session, with a real pointer.
#
# komorebi's focus follows mouse is KWin's own focus policy: the window under the pointer is the one
# that has the focus, and komorebi follows. Checked by moving the pointer over each window in turn,
# then turning it off (the focus stays where it was), and on exit (the user's policy is back in
# `kwinrc`). The pointer is a device made inside the guest (`Pointer`, the program
# `komorebi-pointer`), moved to the middle of a window by pixel.
import time

from komorebi_vm import Checks, Kde, Pointer

UNIT = "komorebi.service"
# the session node's display, which the pointer's two axes span
SCREEN = (1280, 800)

start_all()
machine.wait_for_unit("multi-user.target")

checks = Checks()
kde = Kde.session(machine, scale=SCALE)


def answering() -> bool:
    return kde.run("komorebic monitor-information")[0] == 0


def running() -> bool:
    return kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0


def active_title() -> str:
    wid = kde.active()
    return kde.title(wid) if wid else ""


def focus_policy() -> str | None:
    return kde.kwinrc("Windows", "FocusPolicy")


def kwin_has_read_it() -> None:
    """Waits for KWin to have read the focus policy komorebi wrote.

    The key is in the file at once, and KWin reads it about 200 ms after it is asked to (it waits
    for more changes): a pointer moved before that is moved under the old policy, and a pointer that
    then rests over a window moves nothing.
    """
    time.sleep(1)


def middle(title: str) -> tuple[int, int]:
    frame = kde.rect(kde.wid(title))
    return frame.x + frame.w // 2, frame.y + frame.h // 2


# the session started the module's unit when it logged in: this test starts its own run
kde.systemctl("stop", UNIT)
kde.close_colored_windows()
before = focus_policy()
before_delay = kde.kwinrc("Windows", "DelayFocusInterval")
print(f"     the user's focus policy before: {before or '(not set: click to focus)'}")

print("== focus follows mouse, turned on by command")
kde.systemctl("start", UNIT)
kde.wait_for(answering, timeout=60)
time.sleep(2)
kde.colored_window("win1", "222233")
time.sleep(1)
kde.colored_window("win2", "223322")
kde.wait_for(lambda: kde.managed() == 2, timeout=30)
time.sleep(2)
pointer = Pointer.start(machine, SCREEN, scale=SCALE)
kde.komorebic("focus-follows-mouse", "enable")
checks.check(
    "KWin's focus policy is focus follows mouse, with no delay",
    kde.wait_for(
        lambda: focus_policy() == "1" and kde.kwinrc("Windows", "DelayFocusInterval") == "0",
        timeout=10,
    ),
)

kwin_has_read_it()
first, second = middle("win1"), middle("win2")
print(f"     win1's middle is at {first}, win2's at {second}")
pointer.move(*first)
checks.check(
    "the pointer over win1 gives it the focus",
    kde.wait_for(lambda: active_title() == "win1", timeout=10),
)
pointer.move(*second)
checks.check("over win2, win2", kde.wait_for(lambda: active_title() == "win2", timeout=10))
pointer.move(*first)
checks.check("and back", kde.wait_for(lambda: active_title() == "win1", timeout=10))
checks.check(
    "komorebi followed the compositor",
    kde.wait_for(lambda: kde.focused_title() == "win1", timeout=10),
)

print("== turned off")
kde.komorebic("toggle-focus-follows-mouse")
checks.check(
    "the policy is back to what it was",
    kde.wait_for(lambda: focus_policy() == before, timeout=10),
)
kwin_has_read_it()
pointer.move(*second)
time.sleep(2)
checks.check("the pointer over win2 no longer moves the focus", active_title() == "win1")

print("== turned on again, and put back when komorebi exits")
kde.komorebic("focus-follows-mouse", "enable")
checks.check(
    "the policy is focus follows mouse again",
    kde.wait_for(lambda: focus_policy() == "1", timeout=10),
)
kwin_has_read_it()
# the pointer is resting over win2: it has to go into a window to be followed
pointer.move(*first)
kde.wait_for(lambda: active_title() == "win1", timeout=10)
pointer.move(*second)
checks.check(
    "on again: the pointer over win2 gives it the focus",
    kde.wait_for(lambda: active_title() == "win2", timeout=10),
)
kde.komorebic("stop")
kde.wait_for(lambda: not running(), timeout=30)
kde.close_colored_windows()
checks.check(
    "when komorebi exits the user's focus policy is back",
    kde.wait_for(lambda: focus_policy() == before, timeout=10),
)
checks.check(
    "and so is the delay it had",
    kde.kwinrc("Windows", "DelayFocusInterval") == before_delay,
)

checks.finish()
