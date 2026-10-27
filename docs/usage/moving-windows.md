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

## The pointer

The pointer follows the window that moved, unless
[mouse follows focus](focusing-windows.md#the-pointer) is off.
