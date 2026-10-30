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
