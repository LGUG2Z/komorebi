r"""The screen being locked and unlocked in a real Plasma session, with komorebi running as the
module's user unit. What only a real session can tell: that the lock screen is not tiled (it is a
window of KWin's, on every desktop and in no task list), that nothing komorebi does while the
screen is locked changes the layout, and that when the screen is unlocked komorebi looks at the
desktop again and puts every window where its tile is, with its idea of the focus KWin's.

A real suspend and resume is not tested: the QEMU guest has no way to wake from one. The signals
komorebi waits for, logind's `PrepareForSleep` and the lock screen's `ActiveChanged`, are
tested on private buses by `komorebi/tests/power.rs`, and the desktop it finds on waking against
the fake by `komorebi/tests/wake.rs`.
"""

import time
from typing import Any

from komorebi_vm import Checks, Kde, Rect

UNIT = "komorebi.service"
TITLES = ["sl-a", "sl-b", "sl-c"]
# what a terminal's frame may be off its tile by: up to a cell of its font
CELL = 24
# the daemon's log of the day, without colors, which the unit writes
LOG = "/home/komorebi/.local/share/komorebi/komorebi_plaintext.log.*"


def run(machine: Any, checks: Checks, scale: float) -> None:
    """Runs the checks of the suite `lock-sleep`, which the suite script calls."""
    SCALE = scale  # noqa: N806
    kde = Kde.session(machine, scale=SCALE)

    def daemon_log() -> str:
        return machine.execute(f"cat {LOG}")[1]

    def frames() -> list[Rect]:
        return [kde.rect(kde.wid(title)) for title in TITLES]

    def settled_frames() -> list[Rect]:
        """The frames the windows have once they have stopped moving: the same a second apart.

        A window that opens makes komorebi tile the others again, so the frames are not the tiles
        until the last window has had its turn.
        """
        seen: list[list[Rect]] = []

        def still() -> bool:
            seen.append(frames())
            time.sleep(1 * SCALE)
            return len(seen) >= 2 and seen[-1] == seen[-2]

        kde.wait_for(still, timeout=60)
        return seen[-1]

    def tile_of(title: str) -> Rect:
        """The tile the daemon has for the window now, from its layout."""
        workspace = kde.state()["monitors"]["elements"][0]["workspaces"]["elements"][0]
        for i, container in enumerate(workspace["containers"]["elements"]):
            windows = container["windows"]
            if windows["elements"][windows["focused"]]["details"]["title"] == title:
                tile = workspace["latest_layout"][i]
                return Rect(tile["left"], tile["top"], tile["right"], tile["bottom"])
        raise AssertionError(f"the daemon has no container for {title}")

    def near(a: Rect, b: Rect) -> bool:
        """Whether the frame is the tile, give or take the cell of the terminal's font."""
        return max(abs(a.x - b.x), abs(a.y - b.y), abs(a.w - b.w), abs(a.h - b.h)) <= CELL

    def session_id() -> str:
        """The id of the user's graphical session, which is the one that gets locked."""
        for line in kde.sh("loginctl list-sessions --no-legend").splitlines():
            words = line.split()
            if len(words) > 2 and words[2] == "komorebi":
                shown = kde.sh(f"loginctl show-session {words[0]} -p Type --value")
                if shown.strip() == "wayland":
                    return words[0]
        raise AssertionError("the user has no Wayland session")

    print("== 1. komorebi runs in the session, and has three windows")
    checks.check(
        "the module's unit runs the daemon, which answers komorebic",
        kde.wait_for(lambda: kde.run("komorebic state")[0] == 0, timeout=60),
    )
    # The daemon tiles the windows again when it learns of the panel's room. They are opened after that,
    # so that they are tiled once. The wait is not a check: a session may never take the room, and it
    # ends after a while.
    kde.wait_for_panel_room()
    for title in TITLES:
        kde.spawn(f"foot -T {title} sleep 3010")
        kde.wait_for(lambda title=title: kde.wid(title) != "", timeout=60)
    checks.check(
        "komorebi manages the three", kde.wait_for(lambda: kde.managed(0) == 3, timeout=30)
    )
    checks.check(
        "they are tiled: none is on another",
        kde.wait_for(
            lambda: (
                not any(a.overlaps(b) for i, a in enumerate(frames()) for b in frames()[i + 1 :])
            )
        ),
    )
    tiles = settled_frames()
    sid = session_id()
    print(f"     session {sid}; tiles: {tiles}")

    print("== 2. the screen is locked, and a window is moved while nothing can be drawn")
    kde.sh(f"loginctl lock-session {sid}")
    checks.check(
        "komorebi noticed the lock screen coming up",
        kde.wait_for(lambda: "the screen was locked" in daemon_log(), timeout=30),
    )
    kde.sh(f"kdotool windowmove {kde.wid('sl-b')} 400 300")
    checks.check(
        "while it is locked komorebi has the same three windows: the lock screen is not one",
        kde.managed(0) == 3,
    )
    checks.check(
        "the other two did not move for it",
        kde.rect(kde.wid("sl-a")) == tiles[0] and kde.rect(kde.wid("sl-c")) == tiles[2],
    )

    print("== 3. the screen is unlocked")
    kde.sh(f"loginctl unlock-session {sid}")
    checks.check(
        "komorebi noticed the screen being unlocked",
        kde.wait_for(lambda: "the screen was unlocked" in daemon_log(), timeout=30),
    )
    # the tile it has now, not the one it had: the shell's panel can take room from the windows after
    # the windows were tiled, and the daemon puts every window in the room that is left
    checks.check(
        "the window that was moved is in its tile again",
        kde.wait_for(lambda: near(kde.rect(kde.wid("sl-b")), tile_of("sl-b")), timeout=30),
    )
    checks.check("it still has three windows", kde.managed(0) == 3)
    checks.check(
        "its idea of the focus is KWin's",
        kde.wait_for(
            lambda: kde.active() != "" and kde.title(kde.active()) == kde.focused_title(0),
            timeout=30,
        ),
    )
    checks.check(
        "and it is still running",
        kde.run(f"systemctl --user is-active --quiet {UNIT}")[0] == 0,
    )
