# Example configurations

`komorebi`, and tiling window managers in general, are very complex pieces of software. To make
starting out easier, the repository has an example configuration,
[`docs/komorebi.example.json`](komorebi.example.json), which is kept working: a test loads it with
every change.

## komorebi.json

The example sets some defaults and gives the first monitor eight workspaces, each with a
different layout. This page has the file as it is:

```json
{
  "cross_monitor_move_behaviour": "Insert",
  "default_workspace_padding": 15,
  "default_container_padding": 15,
  "floating_window_aspect_ratio": "Widescreen",
  "floating_layer_behaviour": "Float",
  "resize_delta": 100,
  "ignore_rules": [],
  "floating_applications": [],
  "manage_rules": [],
  "monitors": [
    {
      "workspaces": [
        {
          "name": "I",
          "layout": "BSP"
        },
        {
          "name": "II",
          "layout": "VerticalStack"
        },
        {
          "name": "III",
          "layout": "HorizontalStack"
        },
        {
          "name": "IV",
          "layout": "UltrawideVerticalStack"
        },
        {
          "name": "V",
          "layout": "Rows"
        },
        {
          "name": "VI",
          "layout": "Grid"
        },
        {
          "name": "VII",
          "layout": "RightMainVerticalStack"
        },
        {
          "name": "VIII",
          "layout": "Scrolling"
        }
      ]
    }
  ]
}
```

Put it where the daemon looks for it, `~/.config/komorebi/komorebi.json`, or give it to the daemon
with `komorebic start --config`. [Configuration](usage/configuration.md) explains every key and
where the daemon looks. The settings of a monitor with more than one entry in `monitors`, and of
each workspace, are there too.

### Padding

While you can set the workspace padding (the space between the outer edges of the windows and
the edge of the monitor) and the container padding (the space between each of the tiled windows)
for each workspace on its own, you can also set a default for both of these values that applies
to all workspaces, using `default_workspace_padding` and `default_container_padding`.

### The floating layer

`floating_layer_behaviour` is `Float` in the example: a window that opens while a workspace is on
its floating layer floats, centered, where the default is to tile it. A window that opens while the
workspace is on its tiling layer is tiled either way.

### Layouts

#### BSP

```text
+-------+-----+
|       |     |
|       +--+--+
|       |  |--|
+-------+--+--+
```

#### Vertical Stack

```text
+-------+-----+
|       |     |
|       +-----+
|       |     |
+-------+-----+
```

#### RightMainVerticalStack

```text
+-----+-------+
|     |       |
+-----+       |
|     |       |
+-----+-------+
```

#### Horizontal Stack

```text
+------+------+
|             |
|------+------+
|      |      |
+------+------+
```

#### Columns

```text
+--+--+--+--+
|  |  |  |  |
|  |  |  |  |
|  |  |  |  |
+--+--+--+--+
```

#### Rows

If you have a vertical monitor, I recommend using this layout.

```text
+-----------+
|-----------|
|-----------|
|-----------|
+-----------+
```

#### Ultrawide Vertical Stack

If you have an ultrawide monitor, I recommend using this layout.

```text
+-----+-----------+-----+
|     |           |     |
|     |           +-----+
|     |           |     |
|     |           +-----+
|     |           |     |
+-----+-----------+-----+
```

#### Grid

If you like the `grid` layout in [LeftWM](https://github.com/leftwm/leftwm-layouts) this is almost exactly the same!

The `grid` layout does not support resizing windows tiles.

```text
+-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
|     |     |   |   |   |   |   |   |   |   |   |   |   |   |
|     |     |   |   |   |   |   |   |   |   |   |   |   +---+
+-----+-----+   |   +---+---+   +---+---+---+   +---+---|   |
|     |     |   |   |   |   |   |   |   |   |   |   |   +---+
|     |     |   |   |   |   |   |   |   |   |   |   |   |   |
+-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
  4 windows       5 windows       6 windows       7 windows
```
