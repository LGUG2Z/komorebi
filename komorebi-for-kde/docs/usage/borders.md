# Borders

komorebi puts a colored border around every window. It does not draw it: a KWin effect, KDE
Rounded Corners, draws the border outside each window, in the gap between the tiles, and komorebi
tells the effect how wide it is, how round the corners are and which color each window's border
has. The border around the window that has the focus is blue, and the border around every other
window is gray.

## Installing the effect

The effect is not part of KWin, and KWin finds effects when it starts: log out and in again after
installing it. On NixOS the flake's module installs it with komorebi, which is what
`services.komorebi.effects.borders.enable` does (it is on by default); the effect is built so that
KWin does not load it until komorebi asks.
[`installation.md`](../installation.md) has the module's options.

komorebi loads the effect when it starts and borders are on, and unloads it when it exits or the
borders are turned off. A machine that does not have the effect has no borders: komorebi says so
once in its log, and leaves no room for them.

## Turning borders on and off

[`komorebic border`](../cli/border.md) turns borders on or off. They are on by default.

```sh
komorebic border disable
komorebic border enable
```

## Colors

A window's border has the color of what the window is to the layout:

| Kind | Window | Default |
|---|---|---|
| `single` | the focused window | blue, `66 165 245` |
| `unfocused` | every other window | gray, `128 128 128` |
| `unfocused-locked` | a locked container that does not have the focus | red, `158 8 8` |
| `stack` | a stack that has the focus | green, `0 165 66` |
| `monocle` | a monocled window | pink, `255 51 153` |
| `floating` | a floating window that has the focus | yellow, `245 245 165` |

[`komorebic border-colour`](../cli/border-colour.md) sets the color of a kind, from its red,
green and blue, each from 0 to 255; without `--window-kind` it sets `single`.

```sh
komorebic border-colour 255 128 0
komorebic border-colour --window-kind unfocused 64 64 64
komorebic border-colour --window-kind floating 200 200 0
```

The stock KDE Rounded Corners has one color for the active window and one for the others. The
build the flake installs also takes a color for each window, which is what lets a locked, a
floating, a stack and a monocled window have colors of their own. With the stock effect every
kind is drawn as `single` or `unfocused`, whichever is closer, and komorebi says so once in its
log.

## Width and room

[`komorebic border-width`](../cli/border-width.md) sets how thick the border is, in pixels: 10 by
default. The effect draws at most 20 pixels, and draws the border only where a window has a shadow,
so a window with no shadow has no border.

While the borders are drawn, the layout leaves room for them: every tile gives up the width of the
border on each side, so that the border is drawn in the gap between two windows and does not fall
on the window next to it. [`komorebic border-offset`](../cli/border-offset.md) leaves more room
than the width, in pixels, which the effect draws nothing in. With borders off, or without the
effect, the layout leaves no room at all. A change of the width or the offset reaches the layout
the next time the workspace is tiled again, which [`komorebic retile`](../cli/retile.md) does.

```sh
komorebic border-width 6
komorebic border-offset 2
komorebic retile
```

## In komorebi.json

The same settings can be in [`komorebi.json`](configuration.md#borders): `border`, `border_width`
and `border_offset` are what the three commands above set, and `border_radius` is how round the
corners are, in pixels (10 by default), which has no command. The file sets them when the daemon
starts and when the file changes; a key it leaves out leaves the setting as it is.

```json
{
  "border": true,
  "border_width": 6,
  "border_offset": 2,
  "border_radius": 12
}
```

`border_colours` sets the colors of the kinds of window above, each as red, green and blue or as
a hex color, and leaves out the kinds it does not name. A [theme](themes.md), which the
`theme` key of the file gives too, replaces them.

```json
{
  "border_colours": {
    "single": { "r": 255, "g": 128, "b": 0 },
    "unfocused": "#404040"
  }
}
```

## What is put back

The effect keeps its settings in the user's `kwinrc`, in the group `[Round-Corners]`. komorebi
writes them when it asks for the border, and when it exits or the borders are turned off it puts
back what the user had there, also after a crash: the next run puts them back when it stops.
