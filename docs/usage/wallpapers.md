# Wallpapers

A monitor, or a workspace of it, can have a wallpaper: the picture the monitor shows while that
workspace is in front. A workspace's own wallpaper is shown instead of its monitor's, and a
workspace that has none shows its monitor's. komorebi sets the wallpaper when a workspace comes
into view, however it comes: a [`komorebic`](../cli/focus-workspace.md) command, or a pager, a
shortcut, or an application raising a window of another workspace. It sets it when it starts too,
for the workspace that is in front. A monitor and a workspace with no wallpaper are left alone.

Plasma keeps one wallpaper for each screen, not for each virtual desktop, so komorebi changes the
screen's wallpaper as its workspace changes, as komorebi does on Windows and macOS.

## Setting wallpapers

The wallpapers are in `komorebi.json` ([Configuration](configuration.md)): a `wallpaper` in the
entry of a monitor and one in the entry of a workspace. The first entry of `monitors` is the first
monitor, and so on; the entries of `workspaces` are the workspaces of that monitor, in order. A
file that is read again, because it changed, shows the new wallpaper of the workspace in front at
once.

```json
{
  "monitors": [
    {
      "wallpaper": { "path": "~/Pictures/dunes.png" },
      "workspaces": [
        { "name": "code" },
        { "name": "chat",
          "wallpaper": { "path": "~/Pictures/sea.png",
                         "theme_options": { "theme_variant": "Light", "single_border": "Base09" } } }
      ]
    }
  ]
}
```

A `path` may start with `~` or have environment variables in it (`$HOME`), and may have spaces in
it. The picture fills the screen and is cropped where it does not fit. The monitor has the
`dunes.png` of the example, and the second workspace has `sea.png`.

A request for a wallpaper never holds up komorebi: switching workspaces does not wait for the
Plasma shell. The shell is asked what the screen shows before it is told anything, so a wallpaper
you changed by hand is put back when its workspace is shown again, and a wallpaper that is already
right is not touched.

## The colors of the borders

A wallpaper also gives the borders their colors, unless it says `"generate_theme": false`. The
palette of the picture, made by the generator komorebi for Windows uses, is a Base16 theme, so the
borders of a workspace with a warm picture are warm and those of a workspace with a cool picture
are cool ([Themes](themes.md) has what the six kinds of border take by default). They change with
the workspace, so the colors are right when the workspace comes into view.

`theme_options` choose the variant of the palette, `"theme_variant": "Dark"` (the default) or
`"Light"`, and which color of the palette each kind of border takes, `single_border`,
`stack_border`, `monocle_border`, `floating_border`, `unfocused_border` and
`unfocused_locked_border`, each one of `Base00` to `Base0F`. `bar_accent` is read and not used:
there is no komorebi bar here.

A palette takes seconds to make for a large picture, so each one is kept in
`$XDG_CACHE_HOME/komorebi` (`~/.cache/komorebi`), in a file named for the picture, a hash of its
path, size and time of last change, and the variant. A picture with fewer than eight colors has no
palette: its wallpaper is set, and the borders keep the colors they had.

## When the Plasma shell is not there

The shell is another process, and komorebi may start before it does. komorebi asks again for about
half a minute, with longer waits between the tries, and then says once, in its log, that it is
leaving wallpapers alone until the shell answers. A session with no Plasma shell has no wallpapers
to set, and nothing else in komorebi is affected.
