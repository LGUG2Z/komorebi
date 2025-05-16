# Animations

If you would like to add window movement animations, ensure the following options are
defined in the `komorebi.json` configuration file.

```json
{
  "animation": {
    "enabled": true,
    "duration": 250,
    "fps": 60,
    "style": "EaseOutSine"
  }
}
```

Window movement animations only apply to actions taking place within the same monitor
workspace.

You can optionally set a custom duration in ms with `animation.duration` (default: `250`),
a custom style with `animation.style` (default: `Linear`), and a custom FPS value with
`animation.fps` (default: `60`).

It is important to note that higher `fps` and a longer `duration` settings will result
in increased CPU usage.

This feature is not considered stable, and you may encounter visual artifacts
from time to time.