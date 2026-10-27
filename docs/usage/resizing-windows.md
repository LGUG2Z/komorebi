# Resizing windows

komorebi resizes the focused window by moving the edges of its tile, and its neighbors give up
the room. The size a tile has been given is kept until the workspace is retiled with
`komorebic retile`, so it lasts while windows open and close around it.

## An edge

[`komorebic resize-edge`](../cli/resize-edge.md) (or `resize`) moves one edge of the focused
window: `left`, `right`, `up` or `down`, and `increase` moves it outwards, `decrease` inwards.

```sh
komorebic resize-edge left increase
komorebic resize-edge right decrease
```

An edge with no tile on the other side of it, such as the left edge of the first window in the
BSP layout, does not move. Neither does an edge the layout keeps where it is: the grid layout
has no resizing at all, and in the columns layout a window can only be made wider or narrower.

## An axis

[`komorebic resize-axis`](../cli/resize-axis.md) moves both edges of the focused window along an
axis: `horizontal` the left and the right edge, `vertical` the top and the bottom edge, and
`horizontal-and-vertical` all four.

```sh
komorebic resize-axis horizontal increase
```

## How far

[`komorebic resize-delta`](../cli/resize-delta.md) sets how far a resize goes, in pixels: an
edge moves by half of it. The delta starts at 50, so an edge moves by 25 pixels.

```sh
komorebic resize-delta 100
```

A resize never makes a tile narrower or shorter than a tenth of the monitor's work area, unless
the tile is smaller than that already: a resize that would is carried out as far as it can go.

## Back to the layout

[`komorebic retile`](../cli/retile.md) tiles every monitor again in its layout, without the
sizes the tiles were given. To tile again and keep them, send the `RetileWithResizeDimensions`
message, which has no subcommand of its own.

## Saving and loading the sizes

The sizes the tiles of the focused workspace were given can be saved to a file and loaded back,
on the same workspace or another one with as many windows:

```sh
komorebic save-resize ~/sizes.json
komorebic load-resize ~/sizes.json
```

[`komorebic quick-save-resize`](../cli/quick-save-resize.md) and
[`komorebic quick-load-resize`](../cli/quick-load-resize.md) do the same with one file the daemon
keeps in its temporary directory, `komorebi.quicksave.json` in `$TMPDIR` (or `/tmp`). A path
given to `save-resize` and `load-resize` can name environment variables, which `komorebic`
expands.
