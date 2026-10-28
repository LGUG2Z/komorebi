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
