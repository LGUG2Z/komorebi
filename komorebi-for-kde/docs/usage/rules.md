# Rules

A rule names an application by one of four things about its windows, and says what komorebi does
with them:

- `exe`: the name of the program's executable, such as `kate`;
- `class`: the app id of a Wayland window, such as `org.kde.kate`, or the class of an X11 window;
- `title`: the title of the window;
- `path`: the full path of the executable, such as `/usr/bin/kate`.

[`komorebic state`](../cli/state.md) shows them for every window komorebi has, as the `title`,
`exe`, `class` and `path` of its `details`.

## Ignoring an application

[`komorebic ignore-rule`](../cli/ignore-rule.md) makes komorebi leave the windows of an
application alone. A rule given this way matches a title or a class that starts or ends with what
it names, and an executable or a path that is exactly what it names. The windows it matches are
not tiled when they open, and the tiled windows of the workspaces in front whose executable,
class, title or path is exactly what it names leave the layout.

```sh
komorebic ignore-rule exe steam
komorebic ignore-rule class org.kde.kcalc
komorebic ignore-rule title "Picture-in-Picture"
```

`komorebic float-rule` is the same command. Plasma's desktop and panels (`plasmashell`) and
KRunner are ignored from the start.

[`komorebic manage-rule`](../cli/manage-rule.md) makes komorebi tile the windows of an
application that an ignore rule covers, from the next one that opens.

```sh
komorebic manage-rule class org.kde.plasmashell
```

[`komorebic manage`](managing-windows.md#taking-a-window-out-of-the-layout) does not take in a
window an ignore rule covers: a manage rule does.

## When a rule decides

A rule decides when a window opens. Applications often open a window before they give it its
title, so the title a rule sees then can be a placeholder. When the title of a window komorebi
does not have changes, komorebi asks the rules again, so a window that a title rule ignored while
it had its first title is tiled once its title no longer matches. A window komorebi tiles stays
tiled whatever its title becomes.

## Floating the windows of an application

[`komorebic session-float-rule`](../cli/session-float-rule.md) takes the focused window out of
the layout into the floating windows of its workspace, and adds a rule that floats every window
that opens with the same executable, class and title.
[`komorebic session-float-rules`](../cli/session-float-rules.md) shows those rules, and
[`komorebic clear-session-float-rules`](../cli/clear-session-float-rules.md) removes them: the
windows that float already stay floating.

```sh
komorebic session-float-rule
komorebic session-float-rules
komorebic clear-session-float-rules
```

A window that a float rule matches floats when it opens, and
[`komorebic toggle-float`](floating-windows.md) puts one in the layout.

## How long rules last

The rules given with these commands last until the daemon stops.
[`komorebic global-state`](../cli/global-state.md) shows the ignore and the manage rules the daemon
has, and its data directory.
