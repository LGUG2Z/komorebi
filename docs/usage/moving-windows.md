# Moving windows

komorebi moves the focused window to another tile of its workspace when it is told to. The
window swaps places with the window on that tile, the workspace is tiled again, and the window
that moved keeps the focus.

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

When there is no tile on that side, nothing moves, and the daemon logs "there is no container
in this direction".

## In order

[`komorebic cycle-move`](../cli/cycle-move.md) swaps the focused window with the next or the
previous window in the order of the workspace, and wraps around: `next` from the last window
swaps it with the first.

```sh
komorebic cycle-move next
komorebic cycle-move previous
```

## The pointer

The pointer follows the window that moved, unless
[mouse follows focus](focusing-windows.md#the-pointer) is off.
