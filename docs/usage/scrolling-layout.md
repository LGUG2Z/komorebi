# The Scrolling Layout

In the Scrolling layout every window is a column in a row that is as long as it needs to be, and the screen is a
viewport onto a part of it. A new window is added to the right of the focused one. Nothing is squeezed to make it fit:
the row gets longer, and the screen scrolls to follow your focus.

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "I",
          "layout": "Scrolling",
          "layout_options": {
            "scrolling": {
              "columns": 3,
              "center_focused_column": "Never"
            }
          }
        }
      ]
    }
  ]
}
```

## Column widths

`columns` says how many columns fit across the screen, which is to say that a new column is a `columns`th of the screen
wide. It is `3` unless you say otherwise. It does not say how many windows there are: a lone window is a third of the
screen wide, and is not stretched to fill it.

- Opening or closing a window never changes the width of another window.
- A column that you have resized keeps its width as windows come and go.
- Columns that you have not resized follow `columns`. Changing it with
  [`komorebic scrolling-layout-columns`](../cli/scrolling-layout-columns.md), or crossing the threshold of a
  `layout_options_rules` entry, lays them out again.

## Centring the focused column

`center_focused_column` says when the focused column is put in the middle of the screen. It is one of:

- `"Never"`, the default, which scrolls only as far as it has to.
- `"Always"`, which keeps the focused column in the middle of the screen.
- `"OnOverflow"`, which centres it when it does not fit on the screen together with the column that focus came from.

`true` and `false` are also accepted, as they were before there were three choices: `true` is `"Always"` and `false` is
`"Never"`. Like `columns`, it is set for a workspace, or for a `layout_options_rules` entry.

A lone window is a third of the screen wide, at the left edge. To have it fill the screen, or to take it out of the
layout for a while, use [`komorebic toggle-monocle`](../cli/toggle-monocle.md).

## Columns of several windows

A column can hold more than one window, one above the other, sharing the height of the screen. A new window is always a
column of its own, to the right of the column that has focus. To put windows in the same column:

- [`komorebic consume right`](../cli/consume.md) takes the first window of the column on the right and puts it at the
  bottom of the focused column.
- [`komorebic consume left`](../cli/consume.md) takes the last window of the column on the left and puts it at the top of
  the focused column.
- [`komorebic expel`](../cli/expel.md) takes the focused window out of its column and puts it in a column of its own on
  the right.

- [`komorebic toggle-column-stack`](../cli/toggle-column-stack.md) stacks the windows of the focused column, or spreads
  the focused stack out into a column with a window in each slot. A [stack](../cli/stack.md) is one slot that shows one
  window at a time, and it takes the height of the column. The window that has focus keeps it. The column keeps its width,
  but the heights that its windows were given are not remembered for when you toggle back.

A stack is the stack that you have in every layout, so [`cycle-stack`](../cli/cycle-stack.md),
[`focus-stack-window`](../cli/focus-stack-window.md) and [`stack`](../cli/stack.md) work on it as they always do.
[`unstack`](../cli/unstack.md) takes the window that has focus off the stack and puts it in a column of its own to the
right of the one that it was in.

These only do anything in the Scrolling layout, which is the only one with columns of several windows for now.

The focused window keeps focus, and stays where it is on screen, when you do any of these.

Focus and move commands know about columns:

- [`focus left` and `focus right`](../cli/focus.md) go to the neighbouring column, and to the window in it that had focus
  last. `focus up` and `focus down` go to the windows of the focused column, and stop at the ends, where they carry on
  to another monitor as they always did.
- [`move left` and `move right`](../cli/move.md) move the whole focused column. `move up` and `move down` move the
  window up or down in its column.
- A column keeps its width, and a [stack](../cli/stack.md) is still one tile showing one of its windows, as in any layout.

A column that you resized, or made the full width of the screen, keeps that as windows are added to it or taken out.
A window that you move to another workspace or monitor is a column of its own there, and keeps the width of the column
that it was in.

## Heights

The windows of a column share its height equally. To change that:

- [`komorebic resize-edge up`, `down`](../cli/resize-edge.md) and the [axis](../cli/resize-axis.md) commands make the
  focused window taller or shorter, by half of the [resize delta](../cli/resize-delta.md) for each edge. The other windows
  in the column share what it leaves.

Giving a window a height gives the others in its column an equal share of what it leaves, so there is always something
to take up the difference. No window is made shorter than 100 pixels, and what one window is given cannot
leave the others with less. A window that is alone in its column is as tall as the screen, and a window that is taken
out of its column, or put in another, starts with an equal share.

## Full width and centring

- [`komorebic scrolling-layout-full-width`](../cli/scrolling-layout-full-width.md) makes the focused column as wide as the
  screen, or puts it back to the width that it had.
- [`komorebic center-focus`](../cli/center-focus.md) scrolls the screen so that the focused column is in the middle of
  it, once. `center_focused_column` does it every time that focus moves instead.
- [`komorebic expand`](../cli/expand.md) makes the focused column as wide as it can be without moving another column out
  of view: it takes up the room that the columns on the screen leave. It does nothing when there is none, and in the
  other layouts. With `center_focused_column` set to `"Always"` it makes the column the full width of the screen, or
  puts it back, as `scrolling-layout-full-width` does.

A column that you make the full width of the screen takes the gaps into account, so that it fills the screen exactly.
Nothing else moves to make room for a column that gets wider: the focused column keeps its left edge, and the screen
scrolls only if the column no longer fits.

## Scrolling

The row scrolls only when it has to.

- Focusing a window that is already on screen does not move anything.
- Focusing one that is not scrolls just far enough to bring it in, and no further.
- A window that is wider than the screen starts at the left edge.
- The focused window does not move when windows to its left open, close or change width.

When you close a window, focus goes back to the window that it was opened next to if you have not moved focus since,
and the screen scrolls back to where it was. If you have, focus goes to the window that takes the closed window's place
on the right, or to the one before it if there is none. A window that is closed without having focus does not take focus
with it.

## Resizing

[`komorebic resize-edge`](../cli/resize-edge.md) and [`komorebic resize-axis`](../cli/resize-axis.md) change the width of
the focused column, by half of the [resize delta](../cli/resize-delta.md) for each edge, so resizing the horizontal axis
changes the width by the whole delta. Dragging an edge with the mouse does the same. Resizing the top or bottom edge
changes the [height](#heights) of a window that shares its column with others. A window that is alone in its column is
as tall as the screen, so resizing its height does nothing.

[`komorebic retile`](../cli/retile.md) puts every column back to the width that
[`scrolling-layout-columns`](../cli/scrolling-layout-columns.md) gives, and every window in a column of several back to
an equal share of its height, on the focused workspace of each monitor. The focused column stays where it is on screen,
unless it has to scroll to stay in view.

## Windows with a minimum width

Many apps will not make a window narrower than some width, whatever they are asked. komorebi asks a window how wide it is
once it has been resized, and if it did not get as narrow as its column, the column becomes as wide as the window needs.
With [animations](../common-workflows/animations.md) on that is learned at the next layout, once the animation is done,
and with them off it is straight after the resize. The columns after it move along to make room, and nothing overlaps. A
column never gets narrower again because of this, and never gets wider than the screen.

## Windows that are out of view

A window that scrolls out of view comes to rest at the edge of the work area that it left by, with 5 pixels of it still
on screen. It keeps its size, so nothing has to be resized to bring it back. With
[animations](../common-workflows/animations.md) on, it slides there and back.

A window that is partly in view keeps its size too, and overhangs the edge of the work area.

## Limits

- The Scrolling layout is only supported on a single monitor.
- Switching to another layout and back keeps the widths, but a column whose windows were moved past each other in the
  other layout is made again from the order that they are in.
- Windows resting at the edges have borders too, so you may see a thin line of one at the edge of the screen.
