# Floating windows

A floating window is one of a workspace's windows that is not in its layout: komorebi gives it no
tile and does not move it, and it stays where it is put by hand.

[`komorebic toggle-float`](../cli/toggle-float.md) takes the window that has the focus out of the
layout, and the other windows of the workspace share the work area. The same command on a
floating window puts it back in the layout, at the place of the focused window.

```sh
komorebic toggle-float
```

A floated window keeps its size and position by default. The floating windows of a workspace are
in the daemon's state as `floating_windows`, next to its containers.

## Windows that open floating

With the float override on, every window that opens floats instead of getting a tile.
[`komorebic toggle-float-override`](../cli/toggle-float-override.md) turns it on and off for every
workspace, and
[`komorebic toggle-workspace-float-override`](../cli/toggle-workspace-float-override.md) for the
focused workspace only: the first time it takes the opposite of what every workspace has, and from
then on it follows its own setting.

```sh
komorebic toggle-float-override
komorebic toggle-workspace-float-override
```

The override is in the daemon's state, as `float_override` for every workspace and in each
workspace that has one of its own.

In `komorebi.json` it is `float_override`, for every workspace and in the entry of a workspace.
`float_override_placement` says what is done to the window the override floats: `None` leaves it
where KWin put it, `Center` centers it in the work area and `CenterAndResize` also makes it half
as tall as the work area, with the aspect ratio `floating_window_aspect_ratio` names. A window is
centered only on a workspace that tiles.

## The two layers

A workspace is on one of two layers: its tiles, or its floating windows. Which one decides what
[`komorebic focus`](../cli/focus.md), [`cycle-focus`](../cli/cycle-focus.md),
[`move`](../cli/move.md) and [`resize-edge`](../cli/resize-edge.md) act on. Floating a window puts
the workspace on its floating layer, and giving the focus to a tiled window, with the pointer or a
command, puts it back on its tiling layer. A window that opens and is tiled does the same, and so
does putting a floating window back in the layout.

[`komorebic toggle-workspace-layer`](../cli/toggle-workspace-layer.md) switches the layer by hand.
The windows of the layer it goes to are raised in front of the others, and one of them is given the
focus: the focused floating window, or the focused container's window. Going to the tiles hides the
floating windows, and going back to the floating layer shows them.

```sh
komorebic toggle-workspace-layer
```

On the floating layer:

- `focus left`, `right`, `up` and `down` give the focus to the nearest floating window on that
  side, by where KWin has them. When no floating window is on that side the focus goes to the
  monitor on that side, as it does for tiles, and the command fails where there is none: the
  daemon logs "there is no container or monitor in this direction".
- `cycle-focus next` and `previous` go round the floating windows in the order they were floated,
  and wrap around.
- `move` moves the focused floating window by the [resize delta](resizing-windows.md), 50 pixels
  unless it was set, and not out of the work area.
- `resize-edge` and `resize-axis` change the frame of the focused floating window: `increase` on
  the left or the upper side moves it that way, and on the right or the lower side makes it wider
  or taller by twice the delta; `decrease` does the opposite.
- `preselect-direction` does nothing.

[`komorebic toggle-maximize`](../cli/toggle-maximize.md) maximizes the focused floating window
there, and when it is restored it is a tile, not a floating window.

The layer is `layer` in the state of each workspace, `Tiling` or `Floating`.

What a window that opens while a workspace is on its floating layer gets is
`floating_layer_behaviour` in `komorebi.json`: `Tile`, the default, gives it a tile, and `Float`
floats it, centered in the work area as `floating_layer_placement` says (`Center` by default; the
values are those of `float_override_placement`). The entry of a monitor and the entry of a
workspace can have a `floating_layer_behaviour` of their own: the workspace's comes before the
monitor's, which comes before the one for every monitor.
