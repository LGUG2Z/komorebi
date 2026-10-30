# Shared scenarios and platform extras: what moved where

A suite runs shared scenarios (`komorebi_vm.core.scenarios`) and what only its platform can check
(`komorebi_vm.platforms.kde.extras`), with the checks labelled `[shared]` and `[kde]`. This is the
ledger of the checks a suite stopped making because a shared check makes them, so that none is
dropped without a replacement. A check that only KDE can make stays, as an extra.

## hotplug

The suite had its own sections, then ran the shared scenario "the same, through the neutral
platform". The sections the shared scenario repeats are gone; the rest are `kde_hotplug` steps.

| Was (section) | Now |
|---|---|
| 1, the daemon answers with both monitors; three windows on the left; the monitor's size; tiled | extra `setup` |
| 2, the scale of the left output changes and returns | extra `scale` |
| 3, `move-to-monitor 1`: containers, KWin's `OutputChange`, tiled on the right | extra `sent_by_the_daemon` |
| 4, the user moves a window to the right output | extra `sent_by_the_user` |
| 5, one monitor after the unplug | shared "after a display is unplugged the daemon has one monitor fewer" |
| 5, all three windows on the left monitor | shared "... the daemon still manages every window", and "every window is on the displays that are left" |
| 5, tiled inside the screen that is left, not on each other | shared "... tiled and clear of each other" |
| 5, the daemon alive, its focus KWin's | extra `after` |
| 6, two monitors again | shared "after a display is plugged in the daemon has a monitor for it again" |
| 6, still three windows | shared "... still manages every window" |
| 6, each window somewhere it can be seen | shared "... each where it can be seen" |
| 7, a window moved to the other output again is followed | extra `after` |
| 8, the daemon runs and answers | extra `after` |

Not kept: that the daemon's container count on the left monitor is 3 after the unplug. The shared
scenario checks the managed count and where the windows are, not the count of containers per
monitor. The windows are where the layout puts them, which is what that count stood for.

The suite goes from 25 checks plus the shared ones to 28, which is the shared 11 and 17 of KDE's.

## commands

The suite had its own layout loop (section 2) and then called the shared `layouts`, `sweep` and
`prints`. Nothing the shared scenarios check was removed except as listed; the rest moved to
`kde_commands`, which reads KWin and the daemon's log.

| Was (section) | Now |
|---|---|
| 1, the daemon answers with both outputs; five windows | extra `setup` |
| 2, every layout with five windows: inside the outputs, apart, healthy | shared `layouts`; the extra repeats it at five windows, see below |
| 2, every layout with three, two and one windows | extra `layouts_by_count` |
| 2, each window where the daemon last asked KWin for it | extra `layouts_by_count` (KDE only: reads the daemon's log) |
| 3, the layouts and the sweep of commands | shared `layouts` and `sweep` |
| 3, `replace-configuration` made the window manager from the file | extra `replace_configuration` |
| 3b, `minimize`, `unmanage`, `manage` | extra `handover` |
| 3c, a title rule decides when the window has its title | extra `title_rule` |
| 4, every command that only prints | shared `prints` |
| 4, `komorebic query ...`, `quick-save-resize`, `save-resize` | extra `queries` (candidates for the shared list) |
| 5, every window is where the daemon last asked; a clean stop | extra `at_the_end` |

The extra's layout loop still goes through five windows, for the daemon's log check at that count;
it states its overlap verdict for five windows too, so that one is made twice. Dropping it from the
extra would lose the five-window `where asked` check, which is KDE's.

## drag-resize

The suite's own cases are all KDE's: the pointer put to the pixel, and eight drags of edges and
corners checked against frames, tiles and neighbors. None of them is removed; they moved to
`kde_drag_resize` as `pointer_is_exact` and `edge_cases`, and the suite calls the shared `resize`
and `focus_follows_mouse` after them, as before. The shared `resize` makes one drag of one edge,
which the extra's `win1:right:...` cases also make: that is a repeat by design, because the extra
reads the daemon's tiles and the frames together, and the shared one reads only the system.

One run of the suite failed `win2:left:-240,0` ("frame R moved -96 but was not dragged": the
terminal's frame four cells short of an unchanged tile) and the next passed; not seen again.

## core-e2e

The suite had sections 1 to 11 of its own, written against KWin and the daemon's log, and then ran
the shared scenarios in section 12 through the neutral platform. Sections 2 to 8 made, with KDE's
tools, the checks the shared `tiling`, `focus`, `moving`, `monocle`, `minimizing` and `closing`
make. They are gone; what only KDE can read stayed, as `kde_core` steps after the shared one.

| Was (section) | Now |
|---|---|
| 1, the script is loaded into KWin, which runs it; the daemon answers | extra `start` |
| 2, every window is where the daemon asked for it | extra `windows_where_asked`, after each shared step that moves windows |
| 2, inside the screen; windows apart; none takes the full width; the daemon manages the three | shared `tiling` ("where the layout says", "do not overlap", "share the work area") |
| 2, KWin and the daemon agree on the focus, by the daemon's log | extra `focus_agrees_with_the_log` |
| 2, the daemon's state focuses the window KWin does | shared `tiling` and `focus` |
| 3, 4, the focus by hand, `focus right/left`, `cycle-focus` | shared `focus` |
| 4, moving the focus moved no window | extra `windows_where_asked` after `focus` |
| 5, `move right`, `cycle-move previous`, the focus kept | shared `moving`; extra `windows_where_asked` after it |
| 6, monocle fills the work area, the state says so, back on the tile | shared `monocle` |
| 6, monocle focus behaviour `cycle` | extra `monocle_focus_behaviour` (a candidate for the shared scenario) |
| 7, minimize and restore | shared `minimizing`; extra `windows_where_asked` after it |
| 8, close, the others share the screen, the daemon manages the rest | shared `closing` |
| 8, KWin carried out every command | extra `bridge_did_everything` |
| 9, a clean stop undoes what a monocle did | extra `parking` (opens two windows of its own) |
| 10, a crash with a monocle on leaves nothing stuck | extra `parking` |
| 11, KWin restarts under the daemon | extra `kwin_restart` (the `kwin-restart` suite also covers it) |
| 13, `komorebic stop`: the socket, the script, the window stays | extra `stopped` |

Changed on purpose: the lifecycle sections run after the shared scenarios, on windows of their
own, so that the stop and the crash no longer depend on windows a shared scenario left behind. The
daemon's frame log is read after four shared steps, not after every one.

Also learned while moving it:

- `KWin carried out every command` was one check after section 8, over a log of three windows' worth
  of commands. After every shared step, over the whole corpus, the bridge logs `could not do` for a
  `geo` that targets a window a scenario closed a moment later. That is a race, not a failure, so
  the check now counts only a command for a window that is still open, and runs after each shared
  step so that a real failure names its scenario. A failure there with the window alive is a
  finding.
- After a daemon restart the daemon's focused window can differ from the one KWin has active, and
  activating a window KWin already has active sends no event. `parking` activates another window
  first. Whether the daemon should pick up KWin's active window at start is not decided here.
