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
