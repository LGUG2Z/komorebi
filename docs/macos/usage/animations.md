# Animations

Windows can be animated when they move to their place in a layout. Animations are off by default.

```json
{
  "animation": {
    "enabled": true
  }
}
```

They can also be turned on and off while komorebi is running with the [`komorebic animation`](../cli/animation.md)
command.

## Duration, style and frame rate

Every move takes the same amount of time, whatever the distance, and the animation style decides how that time is
spent: whether a window starts quickly and settles gently, or the other way around.

```json
{
  "animation": {
    "enabled": true,
    "duration": 250,
    "style": "EaseOutCubic",
    "fps": 60
  }
}
```

The duration is set in milliseconds with [`komorebic animation-duration`](../cli/animation-duration.md), the style with
[`komorebic animation-style`](../cli/animation-style.md), and the number of frames per second with
[`komorebic animation-fps`](../cli/animation-fps.md).

Styles that start quickly and settle gently, such as `EaseOutCubic`, `EaseOutQuart`, `EaseOutQuint` and `EaseOutExpo`,
tend to feel the most natural for windows, and the ones further down that list settle more gently than the ones above
them. `Linear`, the default, moves windows at a constant speed and stops them abruptly.

## Different animations for different things

Every option can be set for a kind of animation instead of for all of them. There are two kinds: `movement`, for windows moving to their place in a layout, and `workspace`, for switching
workspaces.

```json
{
  "animation": {
    "enabled": {
      "movement": true,
      "workspace": true
    },
    "duration": {
      "movement": 300,
      "workspace": 400
    },
    "style": {
      "movement": "EaseOutQuint",
      "workspace": "EaseOutCubic"
    }
  }
}
```

## Switching workspaces

With the `workspace` animation on, the windows of the workspace that you leave slide out of the monitor, and the windows
of the workspace that you enter slide in from the other side: from the right when the workspace that you go to has a
higher number, and from the left when it has a lower one.

```json
{
  "animation": {
    "enabled": {
      "workspace": true
    }
  }
}
```

What slides are pictures of the windows, which komorebi takes with the Screen Recording permission that it already asks
for, and not the windows themselves. The real windows are put away, or kept waiting, behind the pictures, and the ones
that you enter are shown where the pictures end up, which the pictures fade away over. The pictures have borders of
their own, in the colours that the windows' borders have or are about to have, which slide along with them. `fps` does not apply: the
pictures are moved by the system.

Switching workspaces is not animated if a window on either workspace is maximized, if there is nothing to slide, and for
a window that komorebi cannot take a picture of, which is put away or shown at once. Only the `focus-workspace` commands
are animated for now. Everything else that changes workspace, such as moving a window to another one, happens at once.

## Good to know

- Windows are moved by asking each app to move them, one frame at a time. The next frame is only started once the last
  one has been applied to every window, so that neighbouring windows stay in step. An app that is slow to answer slows
  all of the windows down a little, rather than falling out of step with them, unless it takes much longer than a few
  frames, in which case it is left to catch up by itself.
- Changing the size of a window is much more work for an app than moving it, because the app has to lay itself out and
  draw itself again. For some apps, browsers and editors in particular, that takes longer than a frame. Windows of those
  apps are only sent new sizes as often as they can keep up with, and carry on moving smoothly in between, so that they
  do not hold the other windows up. They arrive at the right size, but their sizes change in coarser steps on the way.
- Where a window starts from is remembered rather than asked for, so that all of the windows start moving together. It
  is forgotten as soon as an app reports that its window has been moved by something else.
- A window is checked once it arrives. If an app ignored a move, or is not able to be as large or as small as the layout
  wants, the move is repeated a few times and then left alone.
- Some apps animate every move they are asked to make, which gets in the way of komorebi's animation. While a window is
  being animated komorebi turns this off for the app, and puts it back afterwards.
- A window that you grab with the mouse while it is moving is let go of straight away.
- Stacking and unstacking windows, and cycling or focusing a window in a stack, are never animated.
- In the Scrolling layout, windows that scroll out of view slide off the edge of the screen that they left by instead of
  disappearing, and windows that scroll back into view slide in from the same edge.
- To see how an animation went, run komorebi with `--log-level debug`. When the windows stop moving, a line starting with
  `animated for` reports how many frames were drawn, how many were late, and how long the apps took to answer.
