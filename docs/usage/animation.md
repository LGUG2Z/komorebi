# Animation

komorebi can have a window slide to its new place when the layout changes, instead of jumping
there. It does not animate anything itself: a KWin effect, Geometry Change, animates a window
that a program or a script moves or resizes, and komorebi tells it how long the animation lasts.
Animation is off by default.

## Installing the effect

The effect is not part of KWin, and KWin finds effects when it starts: log out and in again after
installing it. On NixOS the flake's module installs it with komorebi, which is what
`services.komorebi.effects.animation.enable` does (it is on by default); the effect is built so
that KWin does not load it until komorebi asks.
[`installation.md`](../installation.md) has the module's options.

komorebi loads the effect when animation is turned on, and unloads it when it is turned off or
komorebi exits. A machine that does not have the effect has no animation: komorebi says so once
in its log, and windows jump to their places.

## Turning it on, and how long it lasts

[`komorebic animation`](../cli/animation.md) turns animation on or off, and
[`komorebic animation-duration`](../cli/animation-duration.md) sets how long it lasts, in
milliseconds: 250 by default.

```sh
komorebic animation enable
komorebic animation-duration 400
komorebic animation disable
```

Both take `--animation-type movement` to set the animation of windows going to their places apart
from the global setting, which is how the settings of one kind of animation win over the global
ones. Movement is the only kind there is. A global setting takes the settings of the kinds away.

```sh
komorebic animation enable --animation-type movement
komorebic animation-duration 600 --animation-type movement
```

[`komorebic animation-fps`](../cli/animation-fps.md) is accepted and does nothing: KWin animates
at the refresh rate of the screen.

## In komorebi.json

The `animation` key of [`komorebi.json`](configuration.md#animation) has the same settings:
`enabled` and `duration`, each either one value for every kind of animation or a value for each
kind (`{ "movement": true }`), and `fps`.

```json
{
  "animation": {
    "enabled": true,
    "duration": 400
  }
}
```

The file sets them when the daemon starts and when the file changes. A file without the key leaves
the settings as they are, and one with it that leaves out `duration` leaves the duration as it is.

## What is animated

What is painted moves; the window does not. The frame KWin reports for a window, and so the frame
komorebi reads back, is the final one from the start of the animation, so komorebi never waits for
an animation to end. The window is not cross-faded as it changes size: it is a move and a resize,
not a change of what the window shows. An interactive move or resize by the user is not animated.
While the effect is loaded, windows that other programs move are animated too.

## What is put back

The effect keeps its settings in the user's `kwinrc`, in the group
`[Effect-kwin4_effect_geometry_change]`. komorebi writes them when it asks for the animation, and
when it exits or animation is turned off it puts back what the user had there.
