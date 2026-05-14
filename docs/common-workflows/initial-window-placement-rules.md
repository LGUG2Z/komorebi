# Initial Window Placement Rules

By default, when a new window is opened in `komorebi`, it is placed **after the
currently focused container**. The `initial_window_placement_rules` workspace
setting allows you to control where new tiled windows are placed in the container
list.

This setting only applies when `window_container_behaviour` is set to `Create`
(the default). It does not apply when set to `Append`.

## Configuration Forms

The `initial_window_placement_rules` setting can be specified in two forms:

### 1. Placement Target (string or integer)

Apply the same placement to all new windows on the workspace. This can be either
a placement strategy name (string) or a 1-based container index (integer):

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

Or with a fixed container index (1-based):

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

This places every new window at the third container position. If the index is out
of bounds, the window falls back to the default `AfterFocused` behaviour.

Available placement strategies:

| Strategy | Description |
|----------|-------------|
| `Primary` | Place at the primary (largest) container position (index 0 for all built-in layouts) |
| `Secondary` | Place at the secondary container position (index 1 for all built-in layouts) |
| `BeforeFocused` | Place before the currently focused container |
| `AfterFocused` | Place after the currently focused container (default behaviour) |
| `Last` | Place at the end of the container list |

### 2. Per-Application Rules (map)

Assign different applications to specific container positions. Map keys can be
placement strategy names or 1-based container indices. Values are matching rules:

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
              "id": "chrome.exe",
              "matching_strategy": "Equals"
            },
            "Secondary": {
              "kind": "Title",
              "id": "Microsoft Teams",
              "matching_strategy": "Equals"
            }
          }
        }
      ]
    }
  ]
}
```

In this example, Chrome windows are always placed at the primary container
position, and windows with "Microsoft Teams" in the title are placed at the
secondary position. All other windows fall back to `AfterFocused`.

## Matching Rules

### Simple Rule

A single matching condition:

```json
{
  "kind": "Exe",
  "id": "chrome.exe",
  "matching_strategy": "Equals"
}
```

### Multiple Rules for the Same Placement (OR logic)

To assign multiple different applications to the same placement target, use an
array. Each element in the array is checked independently — if **any** rule
matches, the window is placed at that target:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" },
      { "kind": "Exe", "id": "firefox.exe", "matching_strategy": "Equals" }
    ]
  }
}
```

This places both Chrome and Firefox at the primary position.

### Composite Rule (AND logic)

To match a window that satisfies **all** conditions, wrap the conditions in an
inner array:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      [
        { "kind": "Exe", "id": "code.exe", "matching_strategy": "Equals" },
        { "kind": "Title", "id": "workspace", "matching_strategy": "Contains" }
      ]
    ]
  }
}
```

This only matches windows where the executable is `code.exe` **and** the title
contains "workspace".

### Mixing OR and AND

You can combine independent rules and composite rules:

```json
{
  "initial_window_placement_rules": {
    "Primary": [
      { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" },
      [
        { "kind": "Exe", "id": "code.exe", "matching_strategy": "Equals" },
        { "kind": "Title", "id": "workspace", "matching_strategy": "Contains" }
      ]
    ]
  }
}
```

This places at the primary position: Chrome (any window), **or** VS Code windows
whose title contains "workspace".

## Primary and Secondary Positions

For all built-in layouts, the primary container is always at index 0 (the
largest pane), and the secondary container is at index 1. This holds true
regardless of layout flip settings — flipping only changes the visual position
of containers on screen, not their index in the container list.

| Layout | Primary (index 0) | Secondary (index 1) |
|--------|-------------------|---------------------|
| **BSP** | Largest split area | Second-largest split |
| **VerticalStack** | Left column | First row in right stack |
| **RightMainVerticalStack** | Right column | First row in left stack |
| **HorizontalStack** | Top row | First column in bottom stack |
| **UltrawideVerticalStack** | Center column | Left column |
| **Columns** | First column | Second column |
| **Rows** | First row | Second row |
| **Grid** | First cell | Second cell |

For custom layouts, the primary and secondary positions are determined by the
`Column::Primary` and `Column::Secondary` definitions in the layout file.

## Rule Evaluation Order

When using the map form with per-application rules, rules are evaluated in
**key order** — alphabetical for placement names, numerical for indices. The
**first matching rule** determines the placement. If no rule matches, the window
falls back to `AfterFocused`.

### Example

```json
{
  "initial_window_placement_rules": {
    "1": { "kind": "Exe", "id": "chrome.exe", "matching_strategy": "Equals" },
    "Primary": { "kind": "Exe", "id": "firefox.exe", "matching_strategy": "Equals" },
    "Secondary": { "kind": "Title", "id": "Teams", "matching_strategy": "Contains" }
  }
}
```

Since `BTreeMap` sorts keys lexicographically, the evaluation order is:

1. `"1"` — numeric strings come before letters
2. `"Primary"` — alphabetical among placement names
3. `"Secondary"`

So when a new window opens:

- **chrome.exe** → matches `"1"` → placed at container index 1 (the first position)
- **firefox.exe** → does not match `"1"`, matches `"Primary"` → placed at the primary position (also index 0, but layout-aware)
- **A window titled "Teams Meeting"** → does not match `"1"` or `"Primary"`, matches `"Secondary"` → placed at the secondary position
- **Any other window** → no match → falls back to `AfterFocused` (after the currently focused container)

### Key ordering detail

Because map keys are sorted as strings, numeric keys sort lexicographically,
not numerically. This means `"10"` sorts before `"2"`. If you mix numeric and
named keys, the order is:

| Key | Sort position |
|-----|---------------|
| `"1"` | 1st (digits before letters) |
| `"10"` | 2nd |
| `"2"` | 3rd |
| `"AfterFocused"` | 4th |
| `"Last"` | 5th |
| `"Primary"` | 6th |
| `"Secondary"` | 7th |

In practice this rarely matters — most configs use only a few keys. But if
order is important, be aware that the first matching rule wins.

### Out of bounds fallback

If a rule matches but the resolved container index is out of bounds (e.g. a
rule targets container `5` but only 3 containers exist), that specific match is
ignored and the window falls back to `AfterFocused`.

## Interaction with Other Settings

- **`preselected_container_idx`**: Manual preselection (via keybinds) takes
  priority over `initial_window_placement_rules`
- **`window_container_behaviour`**: This feature only applies when set to
  `Create`. When set to `Append`, new windows are stacked into the focused
  container regardless of placement rules

## Notes

- Container indices in the configuration are **1-based** for user-friendliness
  (the first container is `1`, not `0`)
- The `Primary` and `Secondary` placement strategies resolve to container indices
  based on the current layout, making rules portable across layout changes
- Focus moves to the newly placed window, consistent with the default behaviour
