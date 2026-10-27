# Layouts

The layout of a workspace turns its work area and the number of its containers into one tile
for each container. Every workspace starts with the BSP layout. A command that changes the
layout tiles the focused workspace again at once.

## The layouts

These are the layouts of `komorebi-layouts`, in the order `cycle-layout next` goes through them:

| Layout | `komorebic` name | The tiles |
|---|---|---|
| BSP | `bsp` | each new tile splits the last one in two, alternately side by side and one above the other |
| Columns | `columns` | side by side, as wide as each other |
| Rows | `rows` | one above the other, as high as each other |
| Vertical stack | `vertical-stack` | the first on the left half, the others stacked on the right |
| Horizontal stack | `horizontal-stack` | the first on the top half, the others side by side below it |
| Ultrawide vertical stack | `ultrawide-vertical-stack` | the first in the middle, the second on the left, the others stacked on the right |
| Grid | `grid` | columns of tiles, as even as the number of containers allows |
| Right main vertical stack | `right-main-vertical-stack` | the first on the right half, the others stacked on the left |
| Scrolling | `scrolling` | columns side by side |

## Changing the layout

[`komorebic change-layout`](../cli/change-layout.md) gives the focused workspace the layout it
names, and [`komorebic cycle-layout`](../cli/cycle-layout.md) the next or the previous one in
the table above:

```sh
komorebic change-layout vertical-stack
komorebic cycle-layout next
komorebic cycle-layout previous
```

`cycle-layout next` goes from the scrolling layout back to BSP. Going backwards,
`komorebi-layouts` leaves the scrolling layout out: `cycle-layout previous` goes from BSP to the
right main vertical stack.

[`komorebic query focused-workspace-layout`](../cli/query.md) prints the layout of the focused
workspace.

## Flipping the layout

[`komorebic flip-layout`](../cli/flip-layout.md) mirrors the tiles of the focused workspace:
`horizontal` swaps left and right, `vertical` top and bottom, and `horizontal-and-vertical`
both. A flip is added to the one the workspace has, so the same flip twice puts the layout
back.

```sh
komorebic flip-layout horizontal
komorebic flip-layout horizontal
```

The containers keep their order, and the directions of [`focus`](focusing-windows.md) and
[`move`](moving-windows.md) are the ones on the screen: after `flip-layout horizontal`,
`focus left` goes to the tile on the left.

## Turning tiling off

[`komorebic toggle-tiling`](../cli/toggle-tiling.md) turns the tiling of the focused workspace
off, and on again. While it is off, komorebi moves none of its windows: they stay where they
are, and can be moved by hand. Windows that open on the workspace are still taken into it, and
when tiling is turned back on, every window is tiled again. `query focused-workspace-layout`
prints `None` while tiling is off.

## Preselecting a tile

[`komorebic preselect-direction`](../cli/preselect-direction.md) reserves the place on one side
of the focused window for the next window that opens on the workspace. The layout gives the
reserved place a tile at once, empty until the window opens.
[`komorebic cancel-preselect`](../cli/cancel-preselect.md) gives the place up.

```sh
komorebic preselect-direction left
komorebic cancel-preselect
```

The place is a position in the order of the workspace, so where its tile is depends on the
layout. A workspace reserves one place at a time: a second `preselect-direction` is ignored
until the window opens or the place is given up. The grid layout has no preselection.
