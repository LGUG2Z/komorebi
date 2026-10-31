# Focusing Windows

Windows can be focused in a direction (left, down, up, right) using the [`komorebic focus`](../cli/focus.md) command.

```
# example showing how you might bind this command

alt + h                 : komorebic focus left
alt + j                 : komorebic focus down
alt + k                 : komorebic focus up
alt + l                 : komorebic focus right
```

Windows can be focused in a cycle direction (previous, next) using the [`komorebic cycle-focus`](../cli/cycle-focus.md)
command.

```
# example showing you might bind this command

alt + shift + oem_4     : komorebic cycle-focus previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-focus next # oem_6 is ]
```

It is possible to attempt to focus the first window, on any workspace, matching an exe using the [
`komorebic eager-focus`](../cli/eager-focus.md) command.

```
# example showing how you might bind this command

win + 1                 : komorebic eager-focus firefox.exe
```

The window at the largest tile can be focused using the [`komorebic promote-focus`](../cli/promote-focus.md) command.

```
# example showing how you might bind this command

alt + return            : komorebic promote-focus
```

The behaviour when attempting to call `komorebic focus` when at the left or right edge of a monitor is determined by
the [`cross_boundary_behaviour`](https://komorebi.lgug2z.com/schema#cross_boundary_behaviour) configuration option.

When set to `Workspace`, the next workspace on the same monitor will be focused.

When set to `Monitor`, the focused workspace on the next monitor in the given direction will be focused.