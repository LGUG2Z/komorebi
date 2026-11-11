# Stacking Windows

Windows can be stacked in a direction (left, down, up, right) using the [`komorebic stack`](../cli/stack.md) command.

```
# example showing how you might bind this command

alt + left              : komorebic stack left
alt + down              : komorebic stack down
alt + up                : komorebic stack up
alt + right             : komorebic stack right
```

Windows can be popped from a stack using the [`komorebic unstack`](../cli/unstack.md) command.

```
# example showing how you might bind this command

alt + oem_1             : komorebic unstack # oem_1 is ;
```

The stack state of the focused window can be toggled using the [`komorebic toggle-stack`](../cli/toggle-stack.md)
command, mimicking Hyprland's `togglegroup` dispatcher. If the focused window is not in a stack, a stack containing only
that window is created; if it is in a stack, it is removed from the stack; and if it is the only window of a stack, the
stack is dissolved.

```
# example showing how you might bind this command

alt + g                 : komorebic toggle-stack
```

Windows can be moved into and out of stacks as they are moved around the workspace using the
[`komorebic move-window-or-stack`](../cli/move-window-or-stack.md) command, mimicking Hyprland's `movewindoworgroup`
dispatcher. If the focused window is in a stack, it is removed from the stack and moved in the given direction; otherwise,
if there is a stack in the given direction, the focused window is moved into it; otherwise, this behaves like a regular
move.

```
# example showing how you might bind this command

alt + shift + h         : komorebic move-window-or-stack left
alt + shift + j         : komorebic move-window-or-stack down
alt + shift + k         : komorebic move-window-or-stack up
alt + shift + l         : komorebic move-window-or-stack right
```

Windows in a stack can be focused in a cycle direction (previous, next) using the [
`komorebic cycle-stack`](../cli/cycle-stack.md) command.

```
# example showing how you might bind this command

alt + oem_4             : komorebic cycle-stack previous # oem_4 is [
alt + oem_6             : komorebic cycle-stack next # oem_6 is ]
```

Windows in a stack can have their positions in the stack moved in a cycle direction (previous, next) using the [
`komorebic cycle-stack-index`](../cli/cycle-stack-index.md) command.

```
# example showing how you might bind this command

alt + shift + oem_4     : komorebic cycle-stack-index previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-stack-index next # oem_6 is ]
```

Windows in a stack can be focused by their index in the stack using the [
`komorebic focus-stack-window`](../cli/focus-stack-window.md) command.

All windows on the focused workspace can be combined into a single stack using the [
`komorebic stack-all`](../cli/stack-all.md) command.

All windows in a focused stack can be popped using the [`komorebic unstack-all`](../cli/unstack-all.md) command.

It is possible to tell the window manager to stack the next opened window on top of the currently focused window by
using the [
`komorebic toggle-workspace-window-container-behaviour`](../cli/toggle-workspace-window-container-behaviour.md) command.
