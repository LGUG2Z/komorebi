# Custom Layouts

Particularly for users of ultrawide monitors, traditional tiling layouts may
not seem like the most efficient use of screen space. If you feel this is the
case with any of the default layouts, you are also welcome to create your own
custom layouts and save them as JSON or YAML.

If you're not comfortable writing the layouts directly in JSON or YAML, you can
use the [komorebi Custom Layout
Generator](https://lgug2z.github.io/komorebi-custom-layout-generator/) to
interactively define a custom layout, and then copy the generated JSON content.

Custom layouts can be loaded on the current workspace or configured for a
specific workspace in the `komorebi.json` configuration file.

```json
{
  "monitors": [
    {
      "workspaces": [
        {
          "name": "personal",
          "custom_layout": "C:/Users/LGUG2Z/my-custom-layout.json"
        },
      ]
    }
  ]
}
```

The fundamental building block of a custom _komorebi_ layout is the Column.

Columns come in three variants:

- **Primary**: This is where your primary focus will be on the screen most of
  the time. There must be exactly one Primary Column in any custom layout.
  Optionally, you can specify the percentage of the screen width that you want
  the Primary Column to occupy.
- **Secondary**: This is an optional column that can either be full height of
  split horizontally into a fixed number of maximum rows. There can be any
  number of Secondary Columns in a custom layout.
- **Tertiary**: This is the final column where any remaining windows will be
  split horizontally into rows as they get added.

If there is only one window on the screen when a custom layout is selected,
that window will take up the full work area of the screen.

If the number of windows is equal to or less than the total number of columns
defined in a custom layout, the windows will be arranged in an equal-width
columns.

When the number of windows is greater than the number of columns defined in the
custom layout, the windows will begin to be arranged according to the
constraints set on the Primary and Secondary columns of the layout.

Here is an example custom layout that can be used as a starting point for your
own:

```yaml
- column: Secondary
  configuration: !Horizontal 2 # max number of rows
- column: Primary
  configuration: !WidthPercentage 50 # percentage of screen
- column: Tertiary
  configuration: Horizontal
```

<!-- TODO: Record a new video -->

[![Watch the tutorial video](https://img.youtube.com/vi/SgmBHKEOcQ4/hqdefault.jpg)](https://www.youtube.com/watch?v=SgmBHKEOcQ4)
