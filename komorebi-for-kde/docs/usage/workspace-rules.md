# Workspace rules

A workspace rule sends the windows of an application to a workspace. It is a [rule](rules.md)
(`exe`, `class`, `title` or `path`) with a place: the index of a monitor and of a workspace, or
the name of a workspace. There are two kinds:

- an **initial** rule sends a window once, when komorebi first sees it: after that the window
  stays wherever you put it;
- a **permanent** rule sends the windows of the application there every time, so that a window
  you take away from that workspace goes back.

The windows are sent when a window opens or is focused, when the configuration is loaded, and when
you ask for it: komorebi looks at the rules at each event about a window it manages, before it
handles it. A window that
opens is in a workspace only after its own event has been handled, so it is sent by the next
event, which is the activation KWin reports a moment later.

Only the windows in view are looked at: the one on top of each container, and the floating
windows. A window that is on its workspace already is left alone. A workspace that does not
exist yet is made, with the ones before it. When the window is on the workspace in front, it
leaves it and is hidden (it is put on the parking desktop, as the back of a stack is), and the
workspace is tiled again without it; either way, it goes to the virtual desktop of its new
workspace, so it is out of view until that workspace is shown, which brings it back and tiles it.
A floating window that goes to another monitor is moved onto that monitor.

## In komorebi.json

A workspace has `workspace_rules` and `initial_workspace_rules`, each a list of
[rules](configuration.md#rules) like the ones in `ignore_rules`:

```json
{
  "monitors": [
    {
      "workspaces": [
        { "name": "web", "layout": "BSP" },
        {
          "name": "code",
          "layout": "BSP",
          "initial_workspace_rules": [
            { "kind": "Exe", "id": "kate", "matching_strategy": "Equals" }
          ],
          "workspace_rules": [
            { "kind": "Class", "id": "org.kde.konsole", "matching_strategy": "Equals" }
          ]
        }
      ]
    }
  ]
}
```

The rules are made again from the file each time it is read, so a rule taken out of the file is
gone, and so are the ones `komorebic` added. The windows that are open are sent where the file
says when it has been read.

## With komorebic

[`initial-workspace-rule`](../cli/initial-workspace-rule.md) and
[`workspace-rule`](../cli/workspace-rule.md) take what the rule names (`exe`, `class`, `title` or
`path`), its text, and the index of the monitor and of the workspace;
[`initial-named-workspace-rule`](../cli/initial-named-workspace-rule.md) and
[`named-workspace-rule`](../cli/named-workspace-rule.md) take the name of the workspace instead,
and add no rule when no workspace has the name. A rule given this way matches a title or a class
that starts or ends with what it names, and an executable or a path that is exactly what it names.
A rule that is there already is not added again.

```sh
komorebic initial-workspace-rule exe kate 0 1
komorebic workspace-rule class org.kde.konsole 0 1
komorebic named-workspace-rule exe thunderbird mail
```

[`enforce-workspace-rules`](../cli/enforce-workspace-rules.md) forgets which windows the initial
rules have sent and looks at the rules again, so that every window of an application goes to the
workspace its initial rule gives. The three clear commands take rules away:
[`clear-workspace-rules`](../cli/clear-workspace-rules.md) by monitor and workspace index,
[`clear-named-workspace-rules`](../cli/clear-named-workspace-rules.md) by name, and
[`clear-all-workspace-rules`](../cli/clear-all-workspace-rules.md) all of them. The first two
also take away the rules of every other workspace of the monitor they are given, and of every
other monitor's workspace with the same index, because of how they pick the rules to keep. Which
rules the daemon has is in the `workspace_rules` of
[`komorebic global-state`](../cli/global-state.md).
