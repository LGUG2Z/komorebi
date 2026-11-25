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

## Workspace transitions

Workspaces can slide in and out of the monitor when you switch between them: the
windows of the workspace you leave slide out of the monitor, and the windows of the
workspace you switch to slide in from the other side, from the right when you go to
a workspace with a higher index (or move right across the edge of a workspace when
`cross_boundary_behaviour` is `Workspace`) and from the left when you go to one with
a lower index (or move left).

It is on when animations are enabled for everything (`"enabled": true`) and ghost
movement is on, and can be turned off in particular with its own animation kind. It
can also be turned on by itself, without the other kinds:

```json
{
  "animation": {
    "enabled": {
      "movement": true,
      "workspace": true
    },
    "ghost_movement": true,
    "duration": {
      "movement": 250,
      "workspace": 350
    },
    "style": {
      "movement": "EaseOutSine",
      "workspace": "EaseOutExpo"
    }
  }
}
```

To turn it off and keep the rest:

```json
{
  "animation": {
    "enabled": true,
    "ghost_movement": true
  }
}
```

becomes

```json
{
  "animation": {
    "enabled": { "movement": true, "workspace": false },
    "ghost_movement": true
  }
}
```

Without `ghost_movement` workspaces switch instantly, as they do without this setting.
The windows never leave the monitor they are on, so no part of them is shown on a
monitor next to it.

The same can be set with `komorebic animation enable --animation-type workspace`,
`komorebic animation-duration 350 --animation-type workspace` and
`komorebic animation-style --style ease-out-expo --animation-type workspace`.