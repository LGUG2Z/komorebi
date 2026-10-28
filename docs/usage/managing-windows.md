# Managing windows

komorebi tiles every normal application window that KWin reports, and leaves dialogs, splash
screens and other windows that are not for tiling alone. The commands here act on the window
that has the focus in KWin, take a window out of the layout or back into it, and pause the
daemon.

## Minimizing and closing

[`komorebic minimize`](../cli/minimize.md) asks KWin to minimize the window that has the focus,
and [`komorebic close`](../cli/close.md) asks it to close the window, as the buttons of its title
bar do.

```sh
komorebic minimize
komorebic close
```

The window leaves the layout when KWin reports that it was minimized or closed, and the other
windows of the workspace share the work area. A minimized window comes back into the layout, in a
container of its own, when it is restored.

## Taking a window out of the layout

[`komorebic unmanage`](../cli/unmanage.md) takes the window that has the focus out of the layout.
It stays open, in the middle of the work area with the size it had, and komorebi leaves it alone
from then on: it does not tile it again, and it does not move it.
[`komorebic manage`](../cli/manage.md), with that window focused, puts it back in the layout,
after the focused window.

```sh
komorebic unmanage
komorebic manage
```

`komorebic manage` also takes in a window komorebi missed, such as one that opened while the
daemon was paused. The daemon forgets an unmanaged window when it closes, and when the daemon
stops.

## Commands while the focused window is not tiled

By default, a command acts on the workspace whatever window has the focus. With
[`komorebic unmanaged-window-operation-behaviour no-op`](../cli/unmanaged-window-operation-behaviour.md),
the daemon ignores every command while the window that has the focus is not one of the windows it
tiles: a floating window, a window it does not manage, or one taken out with
`komorebic unmanage`. The daemon logs "ignoring commands while active window is not managed by
komorebi" for each.

```sh
komorebic unmanaged-window-operation-behaviour no-op
komorebic unmanaged-window-operation-behaviour op
```

The commands that are ignored include `komorebic state`, `komorebic stop` and the command that
sets the behavior back to `op`: give the focus to a tiled window first.

## Pausing

[`komorebic toggle-pause`](../cli/toggle-pause.md) pauses the daemon, and the same command
resumes it. While it is paused, komorebi does not act on what KWin reports, so it moves no
window, and of its commands it carries out only `toggle-pause`, `state` and `stop`. The daemon's
state says whether it is paused, as `is_paused`.

```sh
komorebic toggle-pause
```

What KWin reported while the daemon was paused is not acted on afterwards: a window that opened
in the meantime is not tiled until it is managed with `komorebic manage`.
