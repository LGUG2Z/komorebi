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
