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

## Going round the workspaces

[`komorebic cycle-workspace`](../cli/cycle-workspace.md) shows the next or the previous workspace
of the focused monitor, and wraps around at both ends;
[`komorebic cycle-empty-workspace`](../cli/cycle-empty-workspace.md) shows the next or the previous
one that has no window, if there is one.
[`komorebic cycle-move-to-workspace`](../cli/cycle-move-to-workspace.md) and
[`komorebic cycle-send-to-workspace`](../cli/cycle-send-to-workspace.md) take the focused window to
the next or the previous workspace, with and without showing it.

```sh
komorebic cycle-workspace next
komorebic cycle-empty-workspace previous
komorebic cycle-move-to-workspace next
```

## The last workspace

Each monitor remembers which workspace was in front before the one in front now.
[`komorebic focus-last-workspace`](../cli/focus-last-workspace.md) shows it again, so two in a row
go back and forth between the two.
[`komorebic move-to-last-workspace`](../cli/move-to-last-workspace.md) and
[`komorebic send-to-last-workspace`](../cli/send-to-last-workspace.md) take the focused window
there, with and without showing it.

## Names

[`komorebic workspace-name`](../cli/workspace-name.md) names a workspace, given by the index of its
monitor and its own index, and
[`komorebic ensure-named-workspaces`](../cli/ensure-named-workspaces.md) gives a monitor a workspace
for each name it is given, in order. The name is komorebi's, for the commands that find a
workspace by its name: KWin's desktop keeps the name it has.
[`komorebic focus-named-workspace`](../cli/focus-named-workspace.md) shows the workspace with that
name, on whichever monitor has it, and focuses that monitor. A name no workspace has changes
nothing. `komorebic query focused-workspace-name` prints the name of the workspace in front, or its
index when it has none.

```sh
komorebic ensure-named-workspaces 0 web code mail
komorebic focus-named-workspace mail
```

## Adding and removing workspaces

[`komorebic new-workspace`](../cli/new-workspace.md) adds a workspace after the last one of the
focused monitor and shows it, and [`komorebic ensure-workspaces`](../cli/ensure-workspaces.md)
gives a monitor at least as many workspaces as it is told.
[`komorebic close-workspace`](../cli/close-workspace.md) removes the workspace in front and shows
the one before it, when the workspace has no window and no name and is not the monitor's last.

[`komorebic focus-workspaces`](../cli/focus-workspaces.md) shows the workspace with the same index
on every monitor.

## The layout of another workspace

[`komorebic workspace-layout`](../cli/workspace-layout.md) and
[`komorebic workspace-tiling`](../cli/workspace-tiling.md) set the layout of a workspace, and turn
its tiling off or on, by the index of its monitor and its own index;
[`komorebic named-workspace-layout`](../cli/named-workspace-layout.md) and
[`komorebic named-workspace-tiling`](../cli/named-workspace-tiling.md) find it by its name. The
focused workspace is tiled again at once, and another workspace the next time it is tiled, when it
is shown at the latest.

```sh
komorebic workspace-layout 0 1 columns
komorebic named-workspace-tiling mail disable
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
