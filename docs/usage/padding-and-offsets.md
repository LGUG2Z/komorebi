# Padding and work area offsets

komorebi tiles the windows of a workspace in the work area of its monitor, which is the part of
the monitor that KWin reports free of Plasma's panels. Padding puts gaps around the tiles, and an
offset keeps a part of the work area free of tiles.

## Padding

A workspace has two paddings, in pixels:

- the **workspace padding**, between the edges of the work area and the tiles;
- the **container padding**, around each tile, so that two tiles side by side are twice that far
  apart.

A workspace that has no padding of its own takes 5 pixels for each.

[`komorebic focused-workspace-padding`](../cli/focused-workspace-padding.md) and
[`komorebic focused-workspace-container-padding`](../cli/focused-workspace-container-padding.md)
set the paddings of the focused workspace, and
[`komorebic workspace-padding`](../cli/workspace-padding.md) and
[`komorebic container-padding`](../cli/container-padding.md) those of a workspace given by the
index of its monitor and its own, counted from 0.

```sh
komorebic focused-workspace-padding 20
komorebic focused-workspace-container-padding 10
komorebic workspace-padding 0 1 0
komorebic container-padding 0 1 0
```

[`komorebic adjust-workspace-padding`](../cli/adjust-workspace-padding.md) and
[`komorebic adjust-container-padding`](../cli/adjust-container-padding.md) make a padding of the
focused workspace larger or smaller by a number of pixels. They change a padding the workspace
has of its own, set by one of the commands above: on a workspace that has none, the daemon logs
"there is no workspace padding" or "there is no container padding". A padding is not made
smaller than 0.

```sh
komorebic adjust-workspace-padding increase 5
komorebic adjust-container-padding decrease 5
```

## Work area offsets

An offset is four numbers. The first two move the top left corner of the work area right and
down; the last two make the work area narrower and shorter. So `0 40 0 40` keeps a strip of 40
pixels at the top free, `0 0 0 40` a strip at the bottom, and `40 0 40 0` a strip on the left.

[`komorebic global-work-area-offset`](../cli/global-work-area-offset.md) sets the offset of every
monitor, [`komorebic monitor-work-area-offset`](../cli/monitor-work-area-offset.md) the offset of
one monitor, which it takes instead of the global one, and
[`komorebic workspace-work-area-offset`](../cli/workspace-work-area-offset.md) the offset of one
workspace, which it takes instead of both. Each tiles every monitor again, as
[`komorebic retile`](../cli/retile.md) does.

```sh
komorebic global-work-area-offset 0 40 0 40
komorebic monitor-work-area-offset 1 0 0 0 40
komorebic workspace-work-area-offset 0 2 40 0 40 0
```

A monitor can also have a window-based offset, which applies on top of the other one while the
workspace in front has no more windows than the monitor's limit, 1 unless it is given another.
With a window-based offset, a window that is alone on a wide monitor does not have to take its
whole width. No command sets a monitor's window-based offset: it is part of the configuration of
a monitor.
[`komorebic toggle-window-based-work-area-offset`](../cli/toggle-window-based-work-area-offset.md)
turns it off for the focused workspace, and on again.

```sh
komorebic toggle-window-based-work-area-offset
```

The daemon's state has the offsets as `work_area_offset` (of the window manager, of each monitor
and of each workspace), and a monitor's window-based offset and limit as
`window_based_work_area_offset` and `window_based_work_area_offset_limit`.
