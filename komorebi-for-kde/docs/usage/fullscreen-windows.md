# Fullscreen windows

A window that goes fullscreen, with its program's own key (F11 in many programs), the window
menu or a shortcut, covers its whole monitor, the panels included. komorebi leaves it there. It
never moves or resizes a fullscreen window, whatever happens to the others: a retile, a window
that opens or closes, a [layout](layouts.md) that changes.

The window stays in its container, so the layout keeps a tile for it, and the other windows are
tiled as if it were not fullscreen. The fullscreen window covers them, and they are in their tiles
when it leaves.

When the window leaves fullscreen, KWin gives it the frame it had before. That frame may not be
its tile any more, because windows can have opened or closed meanwhile, so komorebi tiles the
workspace again and the window goes to the tile the layout has for it.

There is no `komorebic` command for fullscreen: it is KWin's, and the program's. A window that is
fullscreen when komorebi starts is left alone the same way.
