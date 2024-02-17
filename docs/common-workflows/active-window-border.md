# Active Window Border

If you would like to add a visual border around the currently focused window,
ensure the following options are defined in the `komorebi.json` configuration
file.

```json
{
  "active_window_border": true,
  "active_window_border_colours": {
    "single": {
      "r": 66,
      "g": 165,
      "b": 245
    },
    "stack": {
      "r": 256,
      "g": 165,
      "b": 66
    },
    "monocle": {
      "r": 255,
      "g": 51,
      "b": 153
    }
  }
}

```

It is important to note that the active window border will only apply to
windows managed by `komorebi`.

This feature is not considered stable and you may encounter visual artifacts
from time to time.

[![Watch the tutorial
video](https://img.youtube.com/vi/7_9D22t7KK4/hqdefault.jpg)](https://www.youtube.com/watch?v=7_9D22t7KK4)
