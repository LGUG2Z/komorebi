# Moving Windows Across Workspaces

Windows can be moved to another workspace on the focused monitor using the [
`komorebic move-to-workspace`](../cli/move-to-workspace.md) command. This command will also move your focus to the
target workspace.

```
# example showing how you might bind this command

alt + shift + 1         : komorebic move-to-workspace 0
alt + shift + 2         : komorebic move-to-workspace 1
alt + shift + 3         : komorebic move-to-workspace 2
```

Windows can be sent to another workspace on the focused monitor using the [
`komorebic send-to-workspace`](../cli/send-to-workspace.md) command. This command will keep your focus on the origin
workspace.

```
# example showing how you might bind this command

alt + shift + 1         : komorebic send-to-workspace 0
alt + shift + 2         : komorebic send-to-workspace 1
alt + shift + 3         : komorebic send-to-workspace 2
```

Windows can be moved to another workspace on the focused monitor in a cycle direction (previous, next) using the [
`komorebic cycle-move-to-workspace`](../cli/cycle-move-to-workspace.md) command. This command will also move your focus
to the target workspace.

```
# example showing how you might bind this command

alt + shift + oem_4     : komorebic cycle-move-to-workspace previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-move-to-workspace next # oem_6 is ]
```

Windows can be sent to another workspace on the focused monitor in a cycle direction (previous, next) using the [
`komorebic cycle-move-to-workspace`](../cli/cycle-move-to-workspace.md) command. This command will keep your focus on
the origin workspace.

```
# example showing how you might bind this command

alt + shift + oem_4     : komorebic cycle-send-to-workspace previous # oem_4 is [
alt + shift + oem_6     : komorebic cycle-send-to-workspace next # oem_6 is ]
```

Windows can be moved or sent to the focused workspace on a another monitor using the [
`komorebic move-to-monitor`](../cli/move-to-monitor.md) and [`komorebic send-to-monitor`](../cli/send-to-monitor.md)
commands.

Windows can be moved or sent to the focused workspace on a monitor in a cycle direction (previous, next) using the [
`komorebic cycle-move-to-monitor`](../cli/cycle-move-to-monitor.md) and [
`komorebic cycle-send-to-monitor`](../cli/cycle-send-to-monitor.md) commands.

Windows can be moved or sent to a named workspace on any monitor (given that all workspace names across all monitors are
unique) using the [`komorebic move-to-named-workspace`](../cli/move-to-named-workspace.md) and [
`komorebic send-to-named-workspace`](../cli/send-to-named-workspace.md) commands
