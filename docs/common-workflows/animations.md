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
workspace. Workspace changes are a separate animation. When enabled, the current workspace
slides off one side of the monitor and the next workspace slides in from the other, in the
same direction Hyprland uses: a higher workspace index enters from the right, a lower index
enters from the left. The travel distance is one monitor width.

Enable it on its own, or turn on global animations and it runs with those settings:

```json
{
  "animation": {
    "enabled": { "workspace": true },
    "duration": { "workspace": 400 },
    "style": { "workspace": [0.05, 0.9, 0.1, 1.05] },
    "fps": 60
  }
}
```

`[0.05, 0.9, 0.1, 1.05]` is Hyprland's default bezier. `komorebic animation`, `animation-duration`,
and `animation-style` accept `--animation-type workspace`.

You can optionally set a custom duration in ms with `animation.duration` (default: `250`),
a custom style with `animation.style` (default: `Linear`), and a custom FPS value with
`animation.fps` (default: `60`). Prefix a setting with `movement`, `transparency`, or
`workspace` to override it for that animation only.

It is important to note that higher `fps` and a longer `duration` settings will result
in increased CPU usage.

This feature is not considered stable, and you may encounter visual artifacts
from time to time.