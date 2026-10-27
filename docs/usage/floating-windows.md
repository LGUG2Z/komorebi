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
