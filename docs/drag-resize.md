# Resizing a tile by dragging its edge

A tiled window can be resized with the pointer: take hold of one of its edges or corners, drag
it, and let go. komorebi then moves the edges of the window's tile by as much as the pointer
moved them, as [`komorebic resize-edge`](cli/resize-edge.md) would, the tiles next to it give up
or take the room, and the window is put on its tile. This page says what komorebi does with a
drag, the rules it keeps to, and why each of them is there.

## What a drag does

1. KWin reports that the user took hold of a window to resize it, and the window manager records
   the frame the window had at that moment.
2. When the user lets go, KWin reports the end of the drag, and the window manager compares the
   frame the user left the window with to the frame it recorded, edge by edge: the left, the top,
   the right and the bottom.
3. For each edge that moved, it asks the layout to move that edge of the tile by the same number
   of pixels, outward or inward. A corner is two edges.
4. The workspace is tiled again: the dragged window goes onto its tile, wherever the user left it,
   and so do its neighbors.

## The rules

- **A drag is measured from where the window was when it began, not from its tile.** A window is
  not always exactly on its tile: a terminal sizes itself to whole cells of its font and is a few
  pixels short, and a window with a minimum size can be larger. Measured against the tile, a drag
  of one edge of such a window would look as if the other edges had moved too, and those would be
  resized as well.
- **Each edge is compared with its own start.** A rectangle in komorebi is a position and a size,
  so the right and the bottom edges are worked out from them. Comparing the sizes instead would
  make a drag of the left edge look like a drag of the right one.
- **The tile of the window that was dragged is resized**, which is not always the focused one: the
  press that began the drag may not have given it the focus yet.
- **Only a tile is resized.** The window of a stack has the tile of the stack, so the drag
  resizes that. A floating window and the window of a monocle are in no tile of the layout, and
  are left as the user made them.
- **An edge with no neighbor does not move.** The left edge of the leftmost tile has nothing to
  take room from: dragging it changes no tile, and the window is put back on its tile.
- **A drag that ends where it began changes nothing**, not even a tile next to it.
- **A drag is granted as far as it can go.** No tile is made narrower or shorter than a tenth of
  the monitor's work area, or than it is already, so a drag further than a neighbor can give
  moves the edge as far as the neighbor allows, and a drag that would shrink the window past that
  shrinks it that far. The same bound applies to `komorebic resize-edge`, and a drag of any
  length is granted, however narrow the dragged tile is.
- **The end of a resize is where the user took the edge.** KWin changes a window's frame once its
  client has drawn at the new size, so when the user lets go the frame can still be short of the
  pointer, and the frames KWin reports after that change it again before the window manager gets to
  the end of the drag, which it may do while it is busy with an earlier event. The bridge keeps the
  frame the user last asked for during the drag, the daemon keeps it apart from the window's frame,
  and the window manager reads that one at the end, whenever it gets there; the window's frame
  follows a moment later. The end of a move is the frame, and so is the end of a resize that had no
  step.

## The layouts

The layouts that move the edges are `komorebi-layouts` from
[komorebi](https://github.com/LGUG2Z/komorebi), at the revision `Cargo.toml` pins. That revision
has the fix of [LGUG2Z/komorebi#1765]: `DefaultLayout::resize` adds the whole adjustment to the
tile and leaves the bound to the window manager, which knows the other tiles. The window manager
grants the largest part of the adjustment that leaves every tile at least as large as the rule
above says (`WindowManager::resize_window`), looking for it by bisection, since the tiles only get
smaller as the adjustment grows. The BSP, columns, rows, vertical stack, right main vertical
stack, horizontal stack, ultrawide vertical stack and scrolling layouts can be resized this way;
the grid layout cannot, and in a layout some edges cannot move, such as the top and the bottom
of a column.

[LGUG2Z/komorebi#1765]: https://github.com/LGUG2Z/komorebi/issues/1765

## How it is tested

- `komorebi/tests/drag_resize.rs`, against the fake desktop: a drag of the split between two
  tiles; a long drag of the narrow right-hand stack of the ultrawide vertical stack; a drag past
  what the neighbor can give; a shrink past what the tile can lose; a drag of a window that is
  short of its tile, which must not move the divider on its other side; a drag of a window that is
  not the focused one; an edge with no neighbor; a drag that changed nothing; a corner; the edge
  of a stack; a floating window and the window of a monocle; a drag whose frame had not got to
  where the user took the edge when the window manager read the end.
- `komorebi/src/kwin/state.rs`: the end of a resize is the frame the user asked for and a frame
  event after it does not change that, the end of a move is the frame, and the frame a drag began
  with is kept while the frame changes.
- `vm-drag-resize`, in a Plasma session with a pointer device: edges and corners of three tiles
  are dragged with the pointer, and the tiles are checked exactly (the dragged edge moved by what
  the pointer moved, the neighbor's with it, nothing else), the frames within a cell of the
  terminal's font.
