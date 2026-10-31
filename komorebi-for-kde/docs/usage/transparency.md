# Transparency

komorebi can make the windows that do not have the focus see-through, so that the one you are
working in stands out. It does not draw anything itself: the opacity of a window is a property
KWin has, which komorebi sets. Transparency is off by default.

## Turning it on

[`komorebic transparency`](../cli/transparency.md) turns transparency on or off,
[`komorebic toggle-transparency`](../cli/toggle-transparency.md) switches it, and
[`komorebic transparency-alpha`](../cli/transparency-alpha.md) sets how opaque a see-through window
is, from 0 (not there at all) to 255 (not see-through): 200 by default.

```sh
komorebic transparency enable
komorebic transparency-alpha 150
komorebic toggle-transparency
```

## In komorebi.json

The same settings are keys of [`komorebi.json`](configuration.md#transparency): `transparency`,
`transparency_alpha` and `transparency_ignore_rules`, a list of rules, as [Rules](rules.md)
explains them, for the windows that are never see-through.

```json
{
  "transparency": true,
  "transparency_alpha": 150,
  "transparency_ignore_rules": [
    { "kind": "Exe", "id": "mpv", "matching_strategy": "Equals" }
  ]
}
```

The file sets them when the daemon starts and when the file changes. A file that leaves out
`transparency` or `transparency_alpha` has the defaults, off and 200, so a change made with a
command does not survive the file changing. The ignore rules add up, as the other rules do.

## Which windows are see-through

On the workspace each monitor shows, the window in front in every container is see-through unless
its container has the focus on the focused monitor. A window on a monitor that is not the focused
one is see-through, whatever its container. On a workspace with a monocle the monocled window is
the only one that counts: it is opaque on the focused monitor and see-through on another. A
workspace that does not tile has no see-through windows, and neither does a window that a rule
keeps out of it: the rules are the `transparency_ignore_rules` of
[`komorebi.json`](configuration.md#transparency), which no command sets.

Windows that are not in front, such as the other windows of a stack, are not on the screen and
are left as they are. With transparency off, every window is opaque.

## What is put back

When komorebi exits, every window it made see-through is opaque again. A window that closes needs
nothing.

## The border

The border effect draws a window from a texture that has the window's opacity in it. The build
of the effect that the flake installs draws a window again when its opacity changes, so the border
and the window change together. With the stock effect a change of opacity alone does not show
until the window changes.
