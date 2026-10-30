# The contract of a Platform

A shared scenario (`nix/tests/vm/lib/komorebi_vm/core/scenarios`) runs on KDE here and on
Windows in kfw's guests, through the `Platform` protocol (`core/platform.py`). A check
that one port answers by how its implementation happens to work, and the other by what
the system says, passes on one port for the wrong reason. This file lists what each member
promises to both ports, and the quirks of each port that a scenario must not lean on. The
docstrings of `Platform` are the contract; this is the audit behind them.

## What each member promises

| Member | Both ports promise | Quirk a scenario must not depend on |
| --- | --- | --- |
| `has(feature)` | `"pointer"`, `"lock"`; a scenario that needs one is skipped, with the reason, where it is false | KDE has `lock` only in a real Plasma session (`session=True`); Windows has both always |
| `tolerance` | how far a frame may be **short** of the one asked for; the frame is never past it | KDE 24 px (a terminal's cells), Windows 2 px (rounding). It bounds what a client rounds to; it is not a measure of a change, so a scenario asks the daemon's layout for the change (`support.layout_rect`) and then that the frame follows it within the tolerance |
| `wait_for(p, t)` | polls `p` until it holds or `t` seconds, scaled for the host | Windows turns a `RuntimeError` of a `komorebic` call in `p` into a false; KDE lets it raise. A predicate answers false for what it cannot tell yet, and relies on neither |
| `komorebic(...)`, `komorebic_status(...)` | `komorebic` raises if the command fails; `_status` returns the exit status and what it printed | the exception type (`AssertionError` on KDE, `RuntimeError` on Windows): a scenario catches `Exception` or nothing |
| `start_komorebi()`, `restart_with(c)` | the daemon starts from a clean state, with the windows that are open taken up | Windows launches it up to three times; KDE once |
| `rewrite_config(c)` | writes the file the daemon watches; returns before the daemon has read it | the delay before a reload is the daemon's (a scenario waits for the state, or `QUIET` for what must not happen) |
| `spawn(title, app_id, exe)` | opens a window and waits until the system has it; raises if it never does | a title is taken as it is and no window has another's; `exe` is a name of letters, digits, `_` and `-`, without an extension (`exe_id(name)` spells it as the daemon names it: `name` on KDE, `name.exe` on Windows) |
| `os_window(c)` | the system's own frame of the window, or `None`; `focused` is the keyboard focus, for at most one window | the frame of a minimized window is wherever the system keeps it; a scenario does not read it |
| `hidden(c)` | true for a window the daemon put away: a stack's other windows, a workspace that is not shown, behind a monocle. False for a window the system does not have, one that is minimized, one that floats | true on KDE for any window on a virtual desktop that is not shown (with more than one output, the desktop of the active output), on Windows for any cloaked window (another Windows desktop included). A scenario that asks for "put away" also asks `state()` where the window is, and counts "in view" through `support.in_view`, which needs the system to have the window |
| `displays()` | each display's rectangle in the desktop's coordinates, left to right, as the system says | displays are not one size (the second Windows display is 1280x800) and a scenario does not compute where the next begins |
| `add_display()`, `remove_display()` | plugs a display in to the right, unplugs the last; return when the system agrees | Windows removes only a display `add_display` plugged in (a hotplug port is used once); KDE enables a disabled output and disables the last enabled one, so a headless session with two outputs can unplug at once. A scenario unplugs only after plugging in, unless its suite started with more than one display |
| `work_area()` | the area of the first display that windows may take, as the system says: the display less panels, docks and the taskbar | KDE asks KWin (a script: its placement area), and answers with the whole output only if KWin does not answer. A headless session has no panel, so there it is the output |
| `close`, `minimize`, `activate`, `move_window` | act on the window as the user would; raise if there is no such window | `close` returns before the window is gone (a scenario waits for `os_window` to be `None`); `move_window` puts the top left of the *visible* frame at (x, y) |
| `lock()`, `unlock()` | lock and unlock the screen and return when it is so | the lock screen is `LogonUI` on Windows and the Plasma greeter on KDE; a scenario asks only what the daemon and the windows do |
| `pointer` | moves in the pixels of the first display; a drag is one press, a glide, one release | a window manager that follows the pointer by a hover delay (Windows) or by its own focus policy (KDE) has to be given time: `focus_follows_mouse.SETTLE` |
| `screenshot(name)` | the display as an image | none |

## Findings of the review, and what was done

1. `work_area()` on KDE was the whole output. **Confirmed.** The headless KWin has no panel so
   no suite could tell, but a scenario's "inside the work area" was weaker than on Windows
   wherever a panel exists. `KdePlatform.work_area()` asks KWin for the placement area of the
   leftmost output (`Kde.screens`, a KWin script like the daemon's bridge uses) and falls back
   to the output. Monocle and maximize now also check that their window stays inside it. To
   verify on KDE: a suite with a panel (`drag-resize`, `run-kde`) and `core-e2e`; the script
   (`workspace.clientArea(KWin.PlacementArea, ...)`) has not been run.
2. `tolerance` is 24 on KDE. **Partly confirmed.** Most uses are right: a frame short of a
   tile by up to 24 px, in a tile hundreds of pixels wide. Not right: `resize_commands` checked
   that a decrease gave back the width within the tolerance, so a daemon that gave back less
   than the increase passed for a terminal. It now compares the daemon's layout and then the
   frame. `inside(area)` takes no slack for the tolerance, because a frame is never past what
   was asked.
3. `hidden()` is true for any window on a desktop that is not shown. **Confirmed as a
   difference, refuted as a source of vacuous passes.** Every scenario that asks for a window to
   be put away also asks the daemon where it is (`workspace_of`, `containers`, `monocle`), and
   a window that was lost is not in view (`support.in_view`). The docstring now says what each
   port is true for. With two outputs on KDE only the first output's windows are certain.
4. `focus_follows_mouse` passed `--implementation windows` and a KWin-sized settle. **Confirmed.**
   Both ports' `komorebic` default to `windows`, so the flag is gone, and the settle is a named
   wait, `SETTLE`, that is longer than either needs.
5. Constants tuned for KDE. **Confirmed for the 400,300 move of `locking`, the absolute room of
   `padding` (5000 and 2000 square pixels), the 60 px drag of `resize`, the first display's
   rectangle in `hotplug` (the work area is the right area) and the quiet period of 3 s written
   in nine places.** They are now derived from the work area or named (`support.QUIET`).
   Checked and found neutral: the `1920`, `foot`, `KWin`, `wayland`, `app_id` and `.exe`
   words in the shared scenarios are in comments, or go through `exe_id` and `spawn`.
   `locking` also did not check that the window moved, so a platform that could not move it while
   locked passed; it does now.
6. Checks that pass for how an implementation answers. **Confirmed**: `spawn` did not raise on
   KDE if the window never came (a "stays unmanaged" check then passed for a window that was
   not there); `close`, `minimize`, `activate` and `move_window` on a window that is not there
   did not raise a message; `displays()` was the size the session was started with and not what
   KWin says; a program name that Windows rejects was shell-quoted into KDE; a title was a regular
   expression on KDE. All fixed. `os_window().focused` is each system's own answer and
   the same in meaning. The frame of a minimized window is not specified (KDE keeps it, Windows
   may park it off screen): no scenario reads it.

## Open items

- `windows_platform.py` (kfw, a draft) builds `Rect(left, top, right, bottom)` for
  `displays()`, `os_window` and `work_area()`. kfde's `Rect` is `(x, y, w, h)`: a display that
  is not at the origin, and every frame, is wrong by its position unless the agent returns the
  width and height under those names. It has to be checked on the first Windows run.
- `inside(area)` has no slack. On Windows the DWM frame might round one pixel past; if the
  first run says so, the slack is a `Platform` member, not a number in the scenarios.
- KDE `hidden()` with two outputs and per-output desktops answers for the desktop of the active
  output only.
- Unverified on KDE, to run: `core-e2e` (work_area, spawn raising, `in_view`, `resize_commands`),
  `hotplug` (displays from KWin, `work_area` after an unplug), `drag-resize` (the drag distance,
  `focus_follows_mouse` with no flag, work area with a panel), `core-lock` (the move while
  locked). On Windows: every scenario above.
