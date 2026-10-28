# Workspaces

Each monitor has workspaces, and shows one of them at a time. A workspace has its own windows
and its own layout. komorebi's workspaces are KWin's virtual desktops: workspace 0 is the first
desktop, workspace 1 the second, and so on, which is why the pager, the overview and KWin's
shortcuts show and switch them too.

komorebi turns on KWin's per-output virtual desktops when it starts, so that each monitor shows
a desktop of its own: switching the workspace of one monitor leaves the others as they are. It
puts the setting back as it was when it stops.

## Showing another workspace

[`komorebic focus-workspace`](../cli/focus-workspace.md) shows a workspace of the focused
monitor, by its number, counted from 0. KWin shows the windows of that workspace and hides the
others, and the window of that workspace that had the focus has it again.

```sh
komorebic focus-workspace 1
komorebic focus-workspace 0
```

A workspace that does not exist yet is created, with the ones before it, and KWin gets a
desktop for each, named "Workspace 2" and so on. A desktop that is there already keeps its
name. The desktops komorebi creates stay when it stops.

## Moving a window to another workspace

[`komorebic move-to-workspace`](../cli/move-to-workspace.md) takes the focused window to another
workspace, puts it after the windows there, and shows that workspace.
[`komorebic send-to-workspace`](../cli/send-to-workspace.md) does the same and stays on the
workspace in front, which is tiled again without the window.

```sh
komorebic move-to-workspace 1
komorebic send-to-workspace 2
```

## Switching with KWin

Anything that switches KWin's desktop switches komorebi's workspace: the pager, KWin's own
shortcuts, the overview, or an application that raises a window of another desktop (KWin then
shows that desktop and focuses the window). komorebi follows, tiles the workspace that came
into view, and asks KWin for nothing in reply. A window moved to another desktop with the
window menu's "Move to Desktop" moves to that workspace.

## The windows that were open before

When komorebi starts, each window that is open goes to the workspace whose desktop it is on,
and each monitor starts on the workspace KWin shows there. A window that is on every desktop
goes to the workspace in front.
