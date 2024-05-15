# Borders

If you would like to add a visual border around both the currently focused window
and unfocused windows ensure the following options are defined in the `komorebi.json`
configuration file.

```json
{
  "border": true,
  "border_width": 8,
  "border_offset": -1,
  "border_style": "System",
  "border_colours": {
    "single": "#42a5f5",
    "stack": "#00a542",
    "monocle": "#ff3399",
    "unfocused": "#808080"
  }
}
```

It is important to note that borders will only apply to windows managed by `komorebi`.

This feature is not considered stable, and you may encounter visual artifacts
from time to time.

[![Watch the tutorial
video](https://img.youtube.com/vi/7_9D22t7KK4/hqdefault.jpg)](https://www.youtube.com/watch?v=7_9D22t7KK4)
