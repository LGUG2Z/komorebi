# Focusing Workspaces

Workspaces on the focused monitor can be focused by their index using the [
`komorebic focus-workspace`](../cli/focus-workspace.md)  command.

If this command is called with an index for a workspace which does not exist, that workspace, and all workspace indexes
required to get to that workspace, will be created.

```
# example showing how you might bind this command

alt + 1                 : komorebic focus-workspace 0
alt + 2                 : komorebic focus-workspace 1
alt + 3                 : komorebic focus-workspace 2
```

Workspaces on the focused monitor can be focused in a cycle direction (previous, next) using the [
`komorebic cycle-workspace`](../cli/cycle-workspace.md) command.

```
# example showing how you might bind this command

alt + shift + oem_4     : komorebic cycle-workspace previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-workspace next # oem_6 is ]
```

Workspaces on other monitors can be focused by both the monitor index and the workspace index using the [
`komorebic focus-monitor-workspace`](../cli/focus-monitor-workspace.md) command.

```
# example showing how you might bind this command

alt + 1                 : komorebic focus-monitor-workspace 0 0
alt + 2                 : komorebic focus-monitor-workspace 0 1 
alt + 3                 : komorebic focus-monitor-workspace 1 0
```

Workspaces on any monitor can be focused by their name (given that all workspace names across all monitors are unique)
using the [`komorebic focus-named-workspace`](../cli/focus-named-workspace.md)  command.

```
# example showing how you might bind this command

alt + c                 : komorebic focus-named-workspace coding
```

Workspaces on all monitors can be set to the same index (emulating single workspaces which span across all monitors)
using the [`komorebic focus-workspaces`](../cli/focus-workspaces.md) command.

```
# example showing how you might bind this command

alt + 1                 : komorebic focus-workspaces 0
alt + 2                 : komorebic focus-workspaces 1
alt + 3                 : komorebic focus-workspaces 2
```

The last focused workspace on the focused monitor can be re-focused using the [
`komorebic focus-last-workspace`](../cli/focus-last-workspace) command.