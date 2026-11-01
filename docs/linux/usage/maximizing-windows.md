# Maximizing windows

[`komorebic toggle-maximize`](../cli/toggle-maximize.md) has KWin maximize the window that has the
focus: it takes the work area of its monitor, the panels left alone, and has no padding around it.
The window leaves the layout while it is maximized, and the same command restores it and puts it
back in a container of its own, at the place in the order of the workspace it was taken from.
A window you maximize yourself, with its title bar, a double click or a shortcut, is treated the
same way: see [A window you maximize](#a-window-you-maximize).

```sh
komorebic toggle-maximize
```

A window that is the only one of its container takes the container with it, and the container is
made again when the window comes back. The tiles that were left are not moved while the window is
maximized: it covers them. A window that opens meanwhile is added to the layout, where a new
window goes (after the focused container, unless the workspace has
[placement rules](../initial-window-placement-rules.md)), and has its tile when the maximized
window is restored. A window that closes while it is maximized takes the maximize with it, and the
other windows are tiled again.

A window in [monocle](monocle.md) that is maximized leaves the monocle. When the monitor of a
maximized window is unplugged, the window goes to a monitor that is left and stays maximized: see
[Changing monitors](changing-monitors.md#a-monitor-is-unplugged).

## A window you maximize

KWin tells komorebi when a window is maximized or restored, whoever did it. A window you maximize
leaves the layout as it does for `toggle-maximize`, and the tiles that were left are not moved
while it is maximized. A window that was floating or in [monocle](monocle.md) leaves that as well.
If another window is maximized already, it goes back to its tile. A window that is already
maximized when komorebi starts, or that opens maximized, is the maximized window from the start.

When you restore the window, with its title bar, a double click or a shortcut, it goes back in a
container of its own at the place it came from, as if `toggle-maximize` had restored it; a window
that was floating comes back as a tile. `toggle-maximize` restores a window you maximized, too.
Only a window that KWin has maximized both ways counts: a window maximized in one direction only,
which a shortcut can do, is a restored window to komorebi.

## What is left alone

While a window is maximized, by `toggle-maximize` or by you, the commands that are about the tiles
of the layout do nothing, and the daemon logs that it ignores them: [`move`](../cli/move.md),
[`cycle-move`](../cli/cycle-move.md), the [stack](stacking-windows.md) commands,
[`promote`](../cli/promote.md) and the other promote commands,
[`resize-edge`](../cli/resize-edge.md) and [`resize-axis`](../cli/resize-axis.md),
[`preselect-direction`](../cli/preselect-direction.md), [`toggle-lock`](../cli/toggle-lock.md),
[`toggle-float`](../cli/toggle-float.md), [`toggle-monocle`](../cli/toggle-monocle.md), and the
commands that move or send a container to another monitor or workspace, such as
[`move-to-monitor`](../cli/move-to-monitor.md) and
[`send-to-workspace`](../cli/send-to-workspace.md). Restore the window first.

[`komorebic focus`](../cli/focus.md) finds no container on any side of the maximized window, so
the focus goes to the monitor on that side, and the command fails where there is none. A
maximized window, or a window in monocle, on the workspace it reaches is given the focus whole.

## In the daemon's state

A workspace with a maximized window has it as `maximized_window`, which is not in its
`containers`, and the index it goes back to as `maximized_window_restore_idx`.
