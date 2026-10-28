# Monitors that change

komorebi follows the outputs KWin has. There is nothing to configure and no command to run: this
page says what komorebi does when a monitor changes.

## A monitor changes size

A monitor changes size when its resolution or its scale changes (in System Settings, under
Display & Monitor, or with `kscreen-doctor output.<name>.scale.1.5`), and when a panel changes
the room the windows have on it: a panel that starts, grows, moves or is hidden. komorebi reads
the size and the work area of every monitor again, and tiles the workspace in front on the
focused monitor in the area it now has.

The windows of the other monitors keep their tiles until something tiles them again.
[`komorebic retile`](../cli/retile.md) does it for every monitor.

KWin has no signal for the room a panel leaves, so komorebi looks at it when a panel opens,
closes or moves, and once a second. A panel that is laid out a moment after komorebi starts is
found within a second.

## A window is moved to another monitor

A window moves to another monitor when something other than komorebi moves it: a KWin shortcut
that sends the window to the next screen, the window menu, a drag with the pointer, or an
application. KWin tells komorebi, which finds every container and floating window that is on
another monitor than the one it has it on, and moves it to the workspace with the same number on
the monitor it is on. The monitors that windows moved between are tiled again.

A window is on the monitor whose area has the middle of its frame. A container is on the monitor
of the window that has the focus in it. A move komorebi made itself, with
[`komorebic move-to-monitor`](../cli/move-to-monitor.md) for one, is in komorebi's state already,
and finds nothing to do.

While the user drags a window with the pointer, komorebi moves nothing: where the window goes is
decided when they let go of it ([moving windows by dragging](moving-windows.md#by-dragging)).

## A monitor is unplugged

KWin takes the windows of an output that goes (it is unplugged, or disabled with
`kscreen-doctor output.<name>.disable`) to the outputs that are left, and puts them where it
likes. komorebi takes the monitor's workspaces over there: every container, and every floating
window, goes to the workspace with the same number on the monitor KWin put its window on, and
the workspaces that changed are tiled again. A [stack](stacking-windows.md) goes as a stack. A
monocle window comes as a tile of its own, and the windows the monocle hid are shown again. A
[maximized window](maximizing-windows.md) stays maximized, on the workspace it goes to; if that
workspace has a maximized window already, it goes back to its tile.

A moment after an output goes or comes back (1.5 seconds), komorebi looks at where every window
is once more, because KWin moves windows about for a while after the outputs change: the windows
it took to the monitor that is left may go back to the output that is plugged in again, and a
change of resolution or scale changes where the middle of a window is.

komorebi keeps the monitor that went, with its workspaces, layouts, paddings and offsets, for
when it comes back, and forgets the [workspace rules](workspace-rules.md) of its workspaces until
then. When no output is left at all, komorebi does nothing.

## A monitor is plugged in

komorebi knows a monitor by the manufacturer, the model and the serial number KWin reports for
the output, or by the output's name when it has no serial number. A monitor komorebi has kept
comes back with what was kept: its workspaces and their settings, and the wallpaper of the
monitor, less the windows that have closed or that are somewhere else now. Its windows were tiled
on other monitors when it went, so it comes back with none of them. The monitor that had the focus
keeps it, and every monitor shows its workspace in front.

A monitor that komorebi has not seen gets workspaces of its own, and the windows that open on it
are tiled there. When `komorebi.json` has an entry for the monitor in
[`display_index_preferences`](configuration.md#monitors-and-workspaces) and the monitor was not
connected when the file was read, it takes the entry when it is plugged in. The monitors are put
at the index the user gave them with
[`komorebic display-index-preference`](../cli/display-index-preference.md), and after the other
monitors otherwise.

## Following it from a program

A [subscriber](subscribing-to-notifications.md) is sent a notification each time komorebi has
dealt with a change of the monitors, whether or not anything changed in the state. Its `event` is
`{"Resize":0}` for a change of size, where the number is the id of the monitor that changed, or 0
when KWin does not say which, and `{"DisplayConnectionChange":0}` for a monitor that was
unplugged or plugged in.
