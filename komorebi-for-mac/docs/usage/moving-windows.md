# Moving Windows

Windows can be moved in a direction (left, down, up, right) using the [`komorebic move`](../cli/move.md) command.

```
# example showing how you might bind this command

alt + shift + h         : komorebic move left
alt + shift + j         : komorebic move down
alt + shift + k         : komorebic move up
alt + shift + l         : komorebic move right
```

Windows can be moved in a cycle direction (previous, next) using the [`komorebic cycle-move`](../cli/cycle-move.md)
command.

```
# example showing how you might bind this command

alt + shift + oem_4     : komorebic cycle-move previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-move next # oem_6 is ]
```

The focused window can be moved to the largest tile using the [`komorebic promote`](../cli/promote.md) command.

```
# example showing how you might bind this command

alt + shift + return    : komorebic promote
```

The behaviour when attempting to call `komorebic move` when at the left or right edge of a monitor is determined by
the [`cross_boundary_behaviour`](https://komorebi.lgug2z.com/schema#cross_boundary_behaviour) configuration option.

When set to `Workspace`, the focused window will be moved to the next workspace on the focused monitor in the given
direction

When set to `Monitor`, the focused window will be moved to the focused workspace on the next monitor in the given
direction.

The behaviour when calling `komorebic move` with `cross_boundary_behaviour` set to `Monitor` can be further refined with
the [`cross_monitor_move_behaviour`](https://komorebi.lgug2z.com/schema#cross_monitor_move_behaviour) configuration
option.

When set to `Swap`, the focused window will be swapped with the window at the corresponding edge of the adjacent monitor

When set to `Insert`, the focused window will be inserted into the focused workspace on the adjacent monitor.

When set to `NoOp`, the focused window will not be moved across a monitor boundary, though focusing across monitor
boundaries will continue to function.