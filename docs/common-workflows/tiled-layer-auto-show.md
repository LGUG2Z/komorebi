# Automatically Show Tiled Windows

To bring the tiled layer forward whenever a tiled window is focused, add this
option at the top level of `komorebi.json`:

```json
{
  "tiled_layer_auto_show": true
}
```

The default is `false`. Setting it to `false`, or removing it, disables the
behaviour when the configuration is reloaded.

When you focus a tiled window through the Windows taskbar, a mouse click, or
keyboard navigation, komorebi raises the other visible tiled windows in the same
workspace. The other tiled windows are placed directly behind the selected app,
which keeps keyboard focus and stays above them. The selected window is not
repositioned or refocused. Window positions, sizes, and the mouse position are
preserved.

Selecting a tiled window from another workspace through the taskbar or Alt+Tab
also brings its tiled siblings forward after the destination workspace is restored.
The same behaviour applies when activation switches to another monitor.

Only the selected window in each stack is raised. Hidden, cloaked, and minimized
windows are skipped. Floating windows and other workspaces are unaffected.
Monocle mode, maximized windows, and workspaces with tiling disabled do not
raise the tiled layer automatically. Windows marked always-on-top retain their
normal Windows behaviour.
