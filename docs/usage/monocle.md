# Monocle

A workspace in monocle shows one window: the container that has the focus takes the whole work
area, and the layout gives the other containers no tile, and their windows are out of sight,
until the monocle is turned off.

[`komorebic toggle-monocle`](../cli/toggle-monocle.md) turns the monocle of the focused workspace
on, for the container that has the focus, and off. The window is put where a window alone in the
workspace would be: the work area less the panels, the workspace padding and the container
padding. Turned off, the container goes back to the place in the order of the workspace it was
taken from, and the layout tiles every container again.

```sh
komorebic toggle-monocle
```

The windows of the other containers are hidden while the monocle is on, the way the windows behind
the front one of a [stack](stacking-windows.md) are, and the front window of each is shown again
when the monocle is turned off. A window that opens meanwhile is added to the layout after the
focused container, and has its tile when the monocle is turned off. When the window in monocle
closes, the monocle is gone and the other windows are shown and tiled again.

## Focusing

Giving the focus to another window does not take the monocle with it: komorebi gives the focus
back to the window in monocle, so that it is the one in front. What focusing in a direction
does meanwhile is set with
[`komorebic monocle-focus-behaviour`](../cli/monocle-focus-behaviour.md):

- `no-op`, the default: no container is on any side of the window in monocle, so
  [`komorebic focus`](../cli/focus.md) goes to the monitor on that side, as it does at the edge of
  a layout, and gives the focus to the window in monocle there if its workspace has one. Where
  there is no monitor on that side it fails, and the daemon logs "there is no container or
  monitor in this direction".
- `cycle`: `komorebic focus left` or `down` makes the previous container the monocle, and
  `right` or `up` the next one.

[`komorebic toggle-monocle-focus-behaviour`](../cli/toggle-monocle-focus-behaviour.md) switches
between the two. [`komorebic cycle-focus`](../cli/cycle-focus.md) always carries the monocle to
the next or the previous container, whatever the behavior is.

```sh
komorebic monocle-focus-behaviour cycle
komorebic toggle-monocle-focus-behaviour
```

## What is left alone

While a window is in monocle, [`komorebic move`](../cli/move.md) and
[`komorebic cycle-move`](../cli/cycle-move.md) are refused, and
[`komorebic toggle-float`](../cli/toggle-float.md) does nothing: the daemon logs that it ignores
them. Turn the monocle off first.

In the daemon's state, a workspace in monocle has the container as `monocle_container`, which
is not one of its `containers`, and the index it goes back to as `monocle_container_restore_idx`.
