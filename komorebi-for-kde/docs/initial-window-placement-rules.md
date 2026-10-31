# Initial window placement rules

By default, when a new window opens, komorebi puts it **after the focused container**. The
`initial_window_placement_rules` setting of a workspace, in its entry in
[`komorebi.json`](usage/configuration.md#monitors-and-workspaces), controls where new tiled windows
go in the list of containers.

The setting only applies when the workspace's `window_container_behaviour` is `Create`, which is
the default. With `Append` (see [Stacking windows](usage/stacking-windows.md)), a new window goes
into the focused container whatever the rules say.

## The two forms

`initial_window_placement_rules` takes a place for every window, or a map from places to the
windows that go there.

### A place for every window

A placement strategy (a string) or a container index (a number, counting from 1):

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "main",
          "initial_window_placement_rules": "Primary"
        }
      ]
    }
  ]
}
```

With a number, every new window goes to that place:

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "main",
          "initial_window_placement_rules": 3
        }
      ]
    }
  ]
}
```

This puts every new window third. If there is no such place, the window goes where it would
have gone without a rule, after the focused container. The place after the last container is
there: `4` is the end of a list of three.

The strategies:

| Strategy | Where the window goes |
|----------|-----------------------|
| `Primary` | The primary (largest) tile: index 0 in every built-in layout |
| `Secondary` | The secondary tile: index 1 in every built-in layout, and none when there is only one container |
| `BeforeFocused` | Before the focused container |
| `AfterFocused` | After the focused container (the default) |
| `Last` | At the end of the list |

### A place for the windows that match

A map from a strategy or a number to the rule that says which windows go there. The key is a
strategy or a place counted from 1, as text:

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "main",
          "initial_window_placement_rules": {
            "Primary": {
              "kind": "Exe",
              "id": "firefox",
              "matching_strategy": "Equals"
            },
            "Last": {
              "kind": "Title",
              "id": "Chat",
              "matching_strategy": "Contains"
            }
          }
        }
      ]
    }
  ]
}
```

Here the windows of Firefox take the primary tile, a window with "Chat" in its title goes to the end
of the list, and every other window goes after the focused container.

## What a rule matches

A rule is the same condition the [ignore and manage rules](usage/rules.md) are made of: a `kind`
(`Exe`, `Class`, `Title` or `Path`), an `id`, and a `matching_strategy`.

### One condition

```json
{
  "kind": "Exe",
  "id": "firefox",
  "matching_strategy": "Equals"
}
```

### Several windows for one place (OR)

An array is a list of conditions, and a window goes to the place when **any** of them matches it:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      { "kind": "Exe", "id": "firefox", "matching_strategy": "Equals" },
      { "kind": "Exe", "id": "chromium", "matching_strategy": "Equals" }
    ]
  }
}
```

This puts Firefox and Chromium in the primary tile.

### A window that matches all of them (AND)

An array inside the array is a condition that needs **all** of its parts:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      [
        { "kind": "Exe", "id": "code", "matching_strategy": "Equals" },
        { "kind": "Title", "id": "workspace", "matching_strategy": "Contains" }
      ]
    ]
  }
}
```

This only matches windows of `code` with "workspace" in their title.

### Both

The two can be mixed in one list:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      { "kind": "Exe", "id": "firefox", "matching_strategy": "Equals" },
      [
        { "kind": "Exe", "id": "code", "matching_strategy": "Equals" },
        { "kind": "Title", "id": "workspace", "matching_strategy": "Contains" }
      ]
    ]
  }
}
```

This puts every window of Firefox in the primary tile, and so every window of `code` that has
"workspace" in its title.

## The primary and secondary tiles

In every built-in layout the primary container is the one at index 0, the largest tile, and the
secondary is the one at index 1. Flipping a layout moves the tiles on the screen and not the
indexes of the containers.

| Layout | Primary (index 0) | Secondary (index 1) |
|--------|-------------------|---------------------|
| **BSP** | The largest split | The second split |
| **VerticalStack** | The left column | The first row of the right stack |
| **RightMainVerticalStack** | The right column | The first row of the left stack |
| **HorizontalStack** | The top row | The first column of the bottom stack |
| **UltrawideVerticalStack** | The center column | The left column |
| **Columns** | The first column | The second column |
| **Rows** | The first row | The second row |
| **Grid** | The first cell | The second cell |
| **Scrolling** | The first column | The second column |

Custom layouts are not supported on KDE yet.

## The order rules are tried in

The rules of the map form are tried in the order of their keys, and the first that matches
decides. A key is compared as text: a number sorts before a name, `10` before `2`, and the names
alphabetically.

```json
{
  "initial_window_placement_rules": {
    "1": { "kind": "Exe", "id": "firefox", "matching_strategy": "Equals" },
    "Primary": { "kind": "Exe", "id": "chromium", "matching_strategy": "Equals" },
    "Secondary": { "kind": "Title", "id": "Chat", "matching_strategy": "Contains" }
  }
}
```

The keys are tried in this order: `"1"`, `"Primary"`, `"Secondary"`. A new window goes:

- of `firefox`: to place 1, the first;
- of `chromium`: to the primary tile, which is also the first place, but is found from the layout;
- with "Chat" in its title: to the secondary tile;
- any other: after the focused container.

A mix of numbers and names sorts like this:

| Key | Position |
|-----|----------|
| `"1"` | 1st |
| `"10"` | 2nd |
| `"2"` | 3rd |
| `"AfterFocused"` | 4th |
| `"Last"` | 5th |
| `"Primary"` | 6th |
| `"Secondary"` | 7th |

Most configurations have a few keys, and the order does not matter. When a window matches two
keys, the first of these decides.

If a rule matches and its place does not exist (a rule for place 5 on a workspace of three
containers), that match is ignored and the window goes after the focused container.

## Other things that decide

- A place that was reserved with [`preselect-direction`](cli/preselect-direction.md) wins over the
  rules, and the rules are back for the next window.
- `window_container_behaviour`: while a workspace has `Append`, because its entry says so, because
  of its `window_container_behaviour_rules`, or because
  [`toggle-workspace-window-container-behaviour`](cli/toggle-workspace-window-container-behaviour.md)
  switched it, the rules are not used.
- The rules decide for every window that gets a container of its own, not only for one that opens:
  one that comes back from being minimized, one that
  [`manage`](cli/manage.md) puts back in the layout, one that
  [`unstack`](cli/unstack.md) takes out of a stack, and one that a
  [workspace rule](usage/workspace-rules.md) or KWin moves to the workspace.
- A locked container keeps its index: a window that a rule puts at the index of a locked container
  goes just after it.
- The container a window opens in takes the focus, as it does without a rule.

## On KDE

This is the same setting as in komorebi for Windows, and it reads the same configuration. What
differs is how a window is named:

- `Exe` is the name of the program (`firefox`, `foot`, `code`). A `.exe` in the `id` is ignored, so
  a `komorebi.json` with `"chrome.exe"` in it works on every platform.
- `Class` is the Wayland `app_id` of the window (the X11 `WM_CLASS` of a window on XWayland), such
  as `org.kde.konsole`.
- `Title` is the title of the window, and `Path` the full path of its executable.

[`komorebic state`](cli/state.md) shows all four for every window. The matching strategies are
those of the [ignore and manage rules](usage/rules.md).

A number is accepted as the whole value of the setting (`"initial_window_placement_rules": 3`),
as this page shows. komorebi for Windows shows the same in its documentation, and as of its commit
83c4e11d its deserializer only reads a number that is quoted: `"3"`.
