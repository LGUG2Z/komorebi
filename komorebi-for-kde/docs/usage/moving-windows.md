# Moving windows

komorebi moves the focused window to another tile of its workspace when it is told to, or when
the window is dragged there. The window swaps places with the window on that tile, the workspace
is tiled again, and the window that moved keeps the focus.

## In a direction

[`komorebic move`](../cli/move.md) swaps the focused window with the window whose tile is on one
side of it: `left`, `right`, `up` or `down`. As with
[`komorebic focus`](focusing-windows.md#in-a-direction), the layout decides which tile is on
which side.

```sh
komorebic move left
komorebic move down
komorebic move up
komorebic move right
```

When there is no tile on that side, the window goes to the monitor on that side, if there is one,
and takes the place nearest the monitor it came from. What it does there is
[`komorebic cross-monitor-move-behaviour`](../cli/cross-monitor-move-behaviour.md): `swap`, the
default, sends the window it takes the place of back to where it came from; `insert` puts it in
among the windows there; `no-op` keeps it where it is.
[`komorebic toggle-cross-monitor-move-behaviour`](../cli/toggle-cross-monitor-move-behaviour.md)
switches between `insert` and `swap`. With no monitor on that side, nothing moves, and the daemon
logs "there is no container or monitor in this direction".

## To another monitor

[`komorebic move-to-monitor`](../cli/move-to-monitor.md) takes the focused window to a monitor, by
its index, and focuses it there; [`komorebic send-to-monitor`](../cli/send-to-monitor.md) does the
same and leaves the focus on the monitor it came from. Their `cycle-` forms take the next or the
previous monitor. [`komorebic move-to-monitor-workspace`](../cli/move-to-monitor-workspace.md) and
[`komorebic send-to-monitor-workspace`](../cli/send-to-monitor-workspace.md) name a workspace of
that monitor too, and [`komorebic move-to-named-workspace`](../cli/move-to-named-workspace.md) and
[`komorebic send-to-named-workspace`](../cli/send-to-named-workspace.md) find it by its name.

```sh
komorebic move-to-monitor 1
komorebic send-to-monitor-workspace 1 2
```

[`komorebic move-workspace-to-monitor`](../cli/move-workspace-to-monitor.md) takes the whole
workspace in front to another monitor, and
[`komorebic swap-workspaces-with-monitor`](../cli/swap-workspaces-with-monitor.md) exchanges the
workspaces of the focused monitor with those of another.

## In order

[`komorebic cycle-move`](../cli/cycle-move.md) swaps the focused window with the next or the
previous window in the order of the workspace, and wraps around: `next` from the last window
swaps it with the first.

```sh
komorebic cycle-move next
komorebic cycle-move previous
```

## To the largest tile

The first tile of a layout is the largest one: the left half in BSP and the vertical stack, the
top half in the horizontal stack, the middle of the ultrawide vertical stack and the right half
in the right main vertical stack. In the columns, rows and scrolling layouts it is the first
column or row. The grid layout is left alone: these commands do nothing there.

[`komorebic promote`](../cli/promote.md) moves the focused window to the first tile, and the
windows that were before it each move one tile on.
[`komorebic promote-swap`](../cli/promote-swap.md) swaps the focused window with the window on
the first tile instead, and from the first tile swaps it back with the window it was last
swapped with. [`komorebic promote-window`](../cli/promote-window.md) promotes the window on one
side of the focused one.

```sh
komorebic promote
komorebic promote-swap
komorebic promote-window left
```

[`komorebic promote-focus`](../cli/promote-focus.md) gives the focus to the window on the first
tile and moves nothing.

## Locking a window in its place

[`komorebic toggle-lock`](../cli/toggle-lock.md) locks the focused window's container at its
place in the order of the workspace, and unlocks it again. Windows that open, close or move
around a locked container leave it where it is: a new window that would go in its place goes
after it. A window moved onto the place of a locked one with `move`, `cycle-move` or
`promote-swap` is locked there instead, since the lock belongs to the place.

```sh
komorebic toggle-lock
```

The `LockMonitorWorkspaceContainer` and `UnlockMonitorWorkspaceContainer` messages lock and
unlock a container by the index of its monitor, its workspace and itself, for a program such as
a status bar. They have no subcommand.

## By dragging

A window can be moved with the pointer too: drag it by its title bar and let go. Where it goes
depends on where the pointer is:

- On the tile of another window of the same monitor, the two windows swap places, and the
  workspace is tiled again. With the
  [window container behavior](stacking-windows.md#windows-that-open-into-a-stack) set to `append`,
  the window joins the stack on that tile instead, in front.
- On another monitor, the window goes there, with the stack it is in, and takes the place of the
  container whose tile it was let go on, which moves one place on. On a monitor with no window it
  is the first container. Both monitors are tiled again.
- Where there is no tile, on a panel for example, the window goes back to its own.

With the [float override](floating-windows.md#windows-that-open-floating) on, a window let go on a
tile of its own monitor floats where it was let go instead. A floating window can be dragged
anywhere and takes no tile. Let go on another monitor, it is one of the floating windows there,
but only when the workspace it came from has a tile: with nothing else on it, komorebi does not
notice that the window changed monitors.

komorebi takes a drag for a move when the window's frame is as wide and as tall as its tile at
the end of it, or when it ends on another monitor. A client that sizes itself in steps, such as
a terminal that keeps to whole cells of its font, can be a few pixels short of its tile: dragging
it on its own monitor swaps nothing, and it stays where it was let go until the workspace is
tiled again, which the next change of focus does.

## The pointer

The pointer follows the window that moved, unless
[mouse follows focus](focusing-windows.md#the-pointer) is off. A drag is the exception: the
pointer is where it was let go, whether the windows swapped, the window joined another or went
back to its tile. Only a window let go on another monitor takes the pointer with it, to the
middle of its tile, unless mouse follows focus is off.
