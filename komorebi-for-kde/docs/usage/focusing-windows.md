# Focusing windows

komorebi gives the focus to another window of the focused workspace when it is told to. It
moves no window to do it: the windows stay on their tiles, and KWin makes the chosen one the
active window.

## In a direction

[`komorebic focus`](../cli/focus.md) focuses the window whose tile is on one side of the
focused one: `left`, `right`, `up` or `down`. Which tile is on which side is the layout's
business. In the BSP layout with three windows, for example, the first window has the left
half, and `focus right` from it goes to the window at the top right.

```sh
komorebic focus left
komorebic focus down
komorebic focus up
komorebic focus right
```

When there is no tile on that side, the focus goes to the monitor on that side, if there is one,
to the tile nearest the monitor the focus came from: `focus right` lands on the leftmost tile of
the monitor on the right. With no monitor there either, nothing happens, and the daemon logs
"there is no container or monitor in this direction".

[`komorebic focus-monitor`](../cli/focus-monitor.md) focuses a monitor by its index,
[`komorebic cycle-monitor`](../cli/cycle-monitor.md) the next or the previous one, and
[`komorebic focus-monitor-at-cursor`](../cli/focus-monitor-at-cursor.md) the one the pointer is
on. [`komorebic eager-focus`](../cli/eager-focus.md) focuses the first window komorebi manages of
an executable, on any workspace of any monitor, and shows its workspace.

```sh
komorebic focus-monitor 1
komorebic eager-focus firefox
```

## In order

[`komorebic cycle-focus`](../cli/cycle-focus.md) focuses the next or the previous window in the
order of the workspace, which is the order in which the windows were opened, each new one
after the window that had the focus. It wraps around: `next` from the last window is the
first.

```sh
komorebic cycle-focus next
komorebic cycle-focus previous
```

## The largest tile

[`komorebic promote-focus`](../cli/promote-focus.md) focuses the window on the first tile of the
layout, which is its largest ([Moving windows](moving-windows.md#to-the-largest-tile)).

## The pointer

By default the pointer moves to the middle of the window that is given the focus. komorebi asks
KWin to do it with its "Move Mouse to Focus" shortcut, so the pointer follows only in a session
that has KWin's global shortcuts (a headless KWin has none).
[`komorebic mouse-follows-focus`](../cli/mouse-follows-focus.md) turns this on or off, and
[`komorebic toggle-mouse-follows-focus`](../cli/toggle-mouse-follows-focus.md) switches between
the two:

```sh
komorebic mouse-follows-focus disable
komorebic toggle-mouse-follows-focus
```

The setting is in the daemon's state as `mouse_follows_focus`, and it lasts until the daemon
stops.

## Binding them to keys

komorebi reads no keys. Bind the commands to keys in Plasma's System Settings, under Keyboard,
Shortcuts, as custom shortcuts that run a command: for example `Meta+H` for
`komorebic focus left` and `Meta+L` for `komorebic focus right`.
