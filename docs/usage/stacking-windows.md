# Stacking windows

A stack is a container with more than one window. The windows share one tile of the layout, and
one of them, the window in front, is in sight: the others are out of the way. Stacking puts two
windows on one tile, so the layout has one tile fewer to give out.

## Putting windows in a stack

[`komorebic stack`](../cli/stack.md) puts the focused window and the window on one side of it in
one stack: `left`, `right`, `up` or `down`. As with
[`komorebic focus`](focusing-windows.md#in-a-direction), the layout decides which tile is on which
side. The focused window ends up in front, and it keeps the focus. Where the layout has no tile on
that side, nothing happens.

```sh
komorebic stack left
komorebic stack down
```

[`komorebic unstack`](../cli/unstack.md) takes the window in front out of its stack and puts it
in a tile of its own after the stack, and the next window of the stack is in front again. A
window that is alone on its tile cannot be unstacked, and the daemon logs "a container must have
at least one window".

```sh
komorebic unstack
```

[`komorebic stack-all`](../cli/stack-all.md) puts every window of the focused workspace in one
stack, in the order they had, with the focused window in front.
[`komorebic unstack-all`](../cli/unstack-all.md) takes every window out of the focused stack,
each onto a tile of its own, and the window that had the focus keeps it.

```sh
komorebic stack-all
komorebic unstack-all
```

A stack moves as one: moving it with [`komorebic move`](moving-windows.md#in-a-direction) or to
another workspace takes every window in it. When the window in front closes, the next window of
the stack comes to the front and has the focus.

## Choosing the window in front

[`komorebic cycle-stack`](../cli/cycle-stack.md) brings the next or the previous window of the
focused stack to the front and gives it the focus; the window that was in front is hidden. It
wraps around: `next` from the last window of the stack brings the first.

```sh
komorebic cycle-stack next
komorebic cycle-stack previous
```

[`komorebic focus-stack-window`](../cli/focus-stack-window.md) brings the window at an index of
the stack to the front, counting from 0 in the order the stack has, which is the order of its
`windows` in [`komorebic state`](../cli/state.md). It works on the monitor the pointer is on,
which it focuses first, so that a program such as a status bar that runs it when one of its tabs
is clicked acts on the stack of its own monitor.

```sh
komorebic focus-stack-window 0
```

[`komorebic cycle-stack-index`](../cli/cycle-stack-index.md) moves the window in front one place
along the stack, swapping it with the window there, and wraps around the same way. It stays in
front and keeps the focus: only the order of the stack changes, which is what `cycle-stack` and
`focus-stack-window` count by.

```sh
komorebic cycle-stack-index next
```

A stack of one window cannot be cycled, and an index the stack does not have cannot be focused:
the daemon logs "there is only one window in this container" or "there is no window in this
container at index 5".

## Windows that open into a stack

By default a window that opens gets a tile of its own. With the window container behavior set to
`append`, a window that opens joins the focused stack instead, in front, and a workspace with no
window still gives it a tile of its own, since there is nothing to join.
[`komorebic toggle-window-container-behaviour`](../cli/toggle-window-container-behaviour.md)
switches every workspace between the two.
[`komorebic toggle-workspace-window-container-behaviour`](../cli/toggle-workspace-window-container-behaviour.md)
does it for the focused workspace only: the first time, it takes the opposite of what every
workspace has, and from then on it follows its own setting.

```sh
komorebic toggle-window-container-behaviour
komorebic toggle-workspace-window-container-behaviour
```

The setting is in the daemon's state, as `new_window_behaviour` for every workspace and as
`window_container_behaviour` in each workspace that has one of its own.

## Where the windows behind go

komorebi does not minimize the windows behind the front one: it moves them to a virtual desktop
that nobody looks at, which it makes when it first needs it and calls `komorebi-park`. A window
there is still open, still on its workspace as far as komorebi is concerned, and still in the
stack, and it comes back to the desktop of its workspace when it is brought to the front, when it
is unstacked, and when komorebi stops. The parking desktop is the last desktop, after the ones
that are workspaces, and is not one of them.

KWin keeps its virtual desktops in its configuration as soon as one is made, so the parking
desktop would be there in the next session if komorebi did not remove it. It does when it stops.
If komorebi did not get to stop (it was killed, or the session ended under it), the next komorebi
finds the parking desktop in KWin's list, sends the windows on it back to the first workspace's
desktop, and removes it. If the parking desktop is removed while komorebi runs, the next hide
makes a new one.

A window on the parking desktop can still be reached by something else. Raising one, with a
click in the task manager, makes KWin show the parking desktop on that monitor, and komorebi
sends the monitor straight back to the workspace it was showing. The window that was raised
becomes the front of its stack when komorebi handles its focus change.
