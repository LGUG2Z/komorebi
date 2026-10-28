# Configuration

komorebi is configured by one file, `komorebi.json`. Many of its settings can also be changed
while the daemon runs, with a `komorebic` command, but the file is where the settings that should
be there every time komorebi starts belong: the workspaces of each monitor and their layouts, the
padding around the tiles, the rules that decide which windows are tiled.

[Example configurations](../example-configurations.md) has a file to start from.

## Where the file is

The daemon reads, in this order:

1. the file `--config` names: `komorebi --config ~/mine.json`, or `komorebic start --config
   ~/mine.json`. Environment variables in the path, such as `$HOME`, are replaced;
2. `komorebi.json` in the config home, which is `$KOMOREBI_CONFIG_HOME` when that is set (it has
   to be a directory) and `~/.config/komorebi` when it is not.

With neither, komorebi runs on its defaults, which are what every setting below says it is when
the file leaves it out. [`komorebic configuration`](../cli/configuration.md) (or `komorebic
config`) prints the path of the file in the config home, when there is one.

On NixOS, [the module's `configFile`](../installation.md#options) gives the daemon a file the
same way `--config` does.

## Changing the file

The daemon watches the file it read. When the file changes, about two seconds later it reads it
again and applies it to the windows it has: it sets the settings, gives each monitor the
workspaces the file lists for it, and tiles every monitor again. The windows stay in the
workspaces they are in. An editor that writes the file in place and one that writes a new file
and renames it over the old one are both followed.

A file that cannot be read, or that has a value of the wrong kind, changes nothing: the daemon
logs the error and keeps what it has. The file is written as JSON that may have comments and
trailing commas.

Reading the file again sets every setting of the daemon from it, and a key the file leaves out is
the default of the setting: a `resize-delta` given with a command is gone when the file changes.
These stay as they were when the file leaves them out: `default_workspace_padding`,
`default_container_padding`, `display_index_preferences`, the four settings of the borders, each
color of `border_colours` the file does not name, the theme and the animation, when the file has no
`animation`. The rules add up, as
[Rules](#rules) says. A workspace the file lists gets its name, padding, offsets, rules and
wallpaper from its entry, and its layout when the entry names one; a workspace the file does not list is left as
it is.

[`komorebic generate-static-config`](../cli/generate-static-config.md) prints the settings the
daemon has now as a `komorebi.json`: a way to start a file from a setup arrived at with commands.
The rules (`ignore_rules`, `manage_rules` and `floating_applications`) are not in what it prints;
the `workspace_rules` and `initial_workspace_rules` of the workspaces that were loaded from a file
are.

## One file for every platform

komorebi for Windows and komorebi for Mac have their own keys, and one `komorebi.json` can serve
all of them. A key that komorebi for KDE does not read is ignored, and so is a key it does not
know: loading never fails because of one. Keys that mean nothing on Plasma, because they are about
how another platform draws or hides windows, are accepted: `titleless_applications`,
`tabbed_applications`, `window_hiding_position` of a monitor, and the keys that only Windows has
(`window_hiding_behaviour`, `border_style`, `stackbar`, and the others of the Windows file).

## The keys

### Settings of the daemon

| Key | Default | What it does |
|---|---|---|
| `resize_delta` | `50` | How many pixels `resize-edge` and `resize-axis` resize by |
| `focus_follows_mouse` | none | `Windows` or `Komorebi` to have KWin give the focus to the window under the pointer ([Focus follows mouse](focus-follows-mouse.md)); the two are the same on KDE. Left out, KWin's focus policy is the user's own |
| `mouse_follows_focus` | `true` | Whether the pointer goes to the window that is given the focus |
| `cross_monitor_move_behaviour` | `Swap` | What `move` does at the edge of a monitor: `Swap`, `Insert` or `NoOp` |
| `cross_boundary_behaviour` | `Monitor` | Where `focus` and `move` go at the edge of a monitor: to the next `Monitor` or the next `Workspace` |
| `unmanaged_window_operation_behaviour` | `Op` | Whether a command is carried out while the focused window is not tiled: `Op` or `NoOp` |
| `window_container_behaviour` | `Create` | What a window that opens gets: a tile of its own (`Create`) or a place in the focused stack (`Append`) |
| `float_override` | `false` | Whether every window that opens floats |
| `float_override_placement` | `None` | What is done to a window the override floats: `None`, `Center` or `CenterAndResize` |
| `floating_layer_behaviour` | `Tile` | What a window that opens while a workspace is on its floating layer gets: `Tile` or `Float` |
| `floating_layer_placement` | `Center` | What is done to a window that `floating_layer_behaviour` floats |
| `monocle_focus_behaviour` | `NoOp` | What `focus` does while a container is in monocle: `NoOp` or `Cycle` |
| `toggle_float_placement` | `CenterAndResize` | What `toggle-float` does to the window it floats: `None`, `Center` or `CenterAndResize` |
| `float_rule_placement` | `None` | The same, for a window that opens and matches `floating_applications` |
| `floating_window_aspect_ratio` | `Standard` | The aspect ratio `CenterAndResize` gives the window: `Standard` (4:3), `Widescreen` (16:9), `Ultrawide` (21:9) or `[width, height]` |

### Padding and offsets

| Key | Default | What it does |
|---|---|---|
| `default_workspace_padding` | `5` | The gap, in pixels, between the edge of the work area and the tiles |
| `default_container_padding` | `5` | The gap around each tile |
| `global_work_area_offset` | none | The part of every monitor's work area that tiles stay out of: `left`, `top`, `right` and `bottom`. `left` and `top` move the edge in; `right` and `bottom` make the area narrower and shorter by that much |

[Padding and work area offsets](padding-and-offsets.md) explains how the three nest. A monitor
(`container_padding`, `workspace_padding`, `work_area_offset`) and a workspace (the same three)
can have values of their own, which come before the default.

### Borders

| Key | Default | What it does |
|---|---|---|
| `border` | `true` | Whether the compositor draws a border around each window (`active_window_border` is the same key under its old name) |
| `border_width` | `10` | How thick the border is, in pixels (`active_window_border_width`) |
| `border_offset` | `0` | How much more room than the width the layout leaves around each window (`active_window_border_offset`) |
| `border_radius` | `10` | How round the corners of the border are, in pixels |
| `border_colours` | see [Borders](borders.md#colors) | The color of the border of each kind of window: `single`, `stack`, `monocle`, `floating`, `unfocused` and `unfocused_locked`, each as `{ "r": 66, "g": 165, "b": 245 }` or `"#42A5F5"` (`active_window_border_colours`). A theme replaces them |
| `theme` | none | A theme that gives every kind of border its color: a Catppuccin theme, a Base16 scheme or a palette of your own ([Themes](themes.md)). It wins over `border_colours` |

[Borders](borders.md) explains what the compositor draws and what the layout leaves room for.
`komorebic border`, `border-width` and `border-offset` change the first three while the daemon
runs.

### Animation

`animation` is an object with these keys:

| Key | Default | What it does |
|---|---|---|
| `enabled` | `false`, required | Whether windows are animated going to their places: `true` or `false` for every kind of animation, or `{ "movement": true }` for the kinds it names |
| `duration` | `250` | How long the animation lasts, in milliseconds, in the same two forms |
| `style` | `Linear` | The easing curve, in the same two forms: a name such as `EaseOutExpo`, or a cubic Bézier as four numbers ([Animation](animation.md#easing)) |
| `fps` | `60` | Accepted and not used: KWin animates at the refresh rate of the screen |

[Animation](animation.md) explains what is animated. A file that has no `animation` leaves the
settings as they are, and one that has it and leaves out `duration` or `style` leaves that one as it
is; a value given for every kind of animation takes the settings of single kinds away.

### Transparency

| Key | Default | What it does |
|---|---|---|
| `transparency` | `false` | Whether the windows that do not have the focus are see-through |
| `transparency_alpha` | `200` | How opaque they are, from 0 to 255 |
| `transparency_ignore_rules` | none | Rules for the windows that are never see-through |

[Transparency](transparency.md) explains which windows are see-through. `transparency_ignore_rules`
is a list of rules, which [Rules](#rules) describes, and has no command.

### Rules

| Key | What it does |
|---|---|
| `ignore_rules` | Windows komorebi leaves alone (`float_rules` is the same key under its old name) |
| `manage_rules` | Windows komorebi tiles whatever the ignore rules say |
| `floating_applications` | Windows that open floating |
| `app_specific_configuration_path` | One `applications.json`, or a list of them, whose rules are added to these ([Application specific configuration](application-specific-configuration.md)) |

Each is a list of rules, which [Rules](rules.md) explains. A rule is an object with `kind` (`Exe`,
`Class`, `Title` or `Path`), `id` and `matching_strategy` (`Equals`, `StartsWith`, `EndsWith`,
`Contains`, `Regex`, or one of the four that negate them: `DoesNotEqual`, `DoesNotStartWith`,
`DoesNotEndWith` and `DoesNotContain`), or a list of such objects, all of which have to match.
Rules in the file add to the ones the daemon has: a rule that is taken out of the file stays
until the daemon restarts. The exception is a workspace's `workspace_rules` and
`initial_workspace_rules`, which the file gives again each time it is read.

### Monitors and workspaces

`monitors` is a list with one entry for each monitor, each with its `workspaces`. The first entry
is for the first monitor, the second for the second, and so on. A monitor that has no entry keeps
what it has.

`display_index_preferences` puts an entry on a monitor by what the monitor is called instead
of by its place: `{ "0": "DP-1", "1": "HDMI-A-1" }` says that the entry at index 0 is for the
monitor `DP-1`. `komorebic monitor-information` shows the names.
A monitor that has no preference takes the first entry nobody's preference names and that no
other monitor has taken.

A monitor entry can also have a `floating_layer_behaviour`, which is the one its workspaces take
unless they have their own, and which comes before the one for every monitor, and a `wallpaper`,
the picture the monitor shows while one of its workspaces is in front, unless that workspace has
one of its own ([Wallpapers](wallpapers.md)).

A monitor takes as many workspaces as its entry lists, and KWin gets a virtual desktop for each,
named for it. A workspace has these keys:

| Key | Default | What it does |
|---|---|---|
| `name` | required | The name `focus-named-workspace` and the other named commands find it by |
| `layout` | none | The layout: `BSP`, `Columns`, `Rows`, `VerticalStack`, `RightMainVerticalStack`, `HorizontalStack`, `UltrawideVerticalStack`, `Grid` or `Scrolling` |
| `tile` | see below | Whether the workspace tiles its windows |
| `layout_flip` | none | The axis the layout is mirrored on: `Horizontal`, `Vertical` or `HorizontalAndVertical` |
| `layout_options` | none | Options of the layout, such as `column_ratios` and `scrolling` |
| `workspace_rules` | none | Rules for the windows that always go to this workspace ([Workspace rules](workspace-rules.md)) |
| `initial_workspace_rules` | none | Rules for the windows that go to this workspace when komorebi first sees them |
| `layout_rules` | none | `{ "3": "Columns" }`: the layout the workspace has once it has 3 containers or more |
| `layout_options_rules` | none | The same, for the layout options |
| `container_padding`, `workspace_padding` | the default | The padding of this workspace |
| `work_area_offset` | none | The offset of this workspace |
| `work_area_offset_rules` | none | `{ "2": { "left": 100, "top": 0, "right": 200, "bottom": 0 } }`: the offset the workspace has once it has 2 containers or more |
| `apply_window_based_work_area_offset` | `true` | Whether the workspace takes its monitor's window-based offset |
| `window_container_behaviour` | the default | What a window that opens on this workspace gets: `Create` or `Append` |
| `window_container_behaviour_rules` | none | `{ "2": "Append" }`: the behavior the workspace has once it has 2 containers or more |
| `float_override` | the default | Whether every window that opens on this workspace floats |
| `floating_layer_behaviour` | the default | What a window that opens while this workspace is on its floating layer gets: `Tile` or `Float` |
| `wallpaper` | none | The picture the monitor shows while this workspace is in front, instead of its monitor's: `{ "path": "~/Pictures/sea.png" }`, and `generate_theme` and `theme_options` for the borders' colors ([Wallpapers](wallpapers.md)) |
| `initial_window_placement_rules` | none | Where in the list of containers a window that opens on this workspace goes: a place, or a place for each kind of window ([Initial window placement rules](../initial-window-placement-rules.md)) |

A workspace that names no `layout` and does not say `"tile": true` does not tile: its windows
stay where they open. A workspace with `layout_rules` tiles. Otherwise a workspace tiles.

The highest of the rules that the number of containers reaches wins. A layout rule changes the
layout of the workspace and leaves it there when the number falls below the threshold again, so a
rule at 1 is how to say what the layout is when there is one window; the offset and the layout
options of a rule are the ones the workspace has while it has that many containers.

The window container behavior rules are the exception: while the workspace has fewer containers
than the lowest threshold it has no behavior of its own and takes the one for every workspace,
and a workspace that has rules takes its behavior from them alone, so
`toggle-workspace-window-container-behaviour` does not outlast the next time it is tiled.

### Layout options for every workspace

`layout_defaults` gives the layout options and the layout options rules of a layout to every
workspace that has that layout and sets neither `layout_options` nor `layout_options_rules`:

```json
{
  "layout_defaults": {
    "Columns": { "layout_options": { "column_ratios": [0.6] } },
    "Scrolling": { "layout_options": { "scrolling": { "columns": 2 } } }
  }
}
```

A workspace that sets either of its own has none of the defaults of its layout, not even for the
setting it leaves out.

## Layout rules from the command line

The layout rules of a workspace can be added and taken away while the daemon runs:

```sh
komorebic workspace-layout-rule 0 1 3 columns
komorebic named-workspace-layout-rule mail 2 rows
komorebic clear-workspace-layout-rules 0 1
komorebic clear-named-workspace-layout-rules mail
```

[`workspace-layout-rule`](../cli/workspace-layout-rule.md) takes the index of the monitor, the
index of the workspace, the number of containers and the layout;
[`named-workspace-layout-rule`](../cli/named-workspace-layout-rule.md) takes the name of the
workspace instead. A rule for a count that has one replaces it. The clear commands take every
rule of the workspace away and leave it with the layout it has.

## The schema

The Rust types that read `komorebi.json` also describe it as a JSON Schema, which an editor can
check the file against, and which the NixOS module is typed from. [`schema.json`](../../schema.json)
in the repository is the schema of `komorebi.json`, [`schema.asc.json`](../../schema.asc.json) the
schema of an [`applications.json`](application-specific-configuration.md), and the commands print
them (and two more), without a daemon:

```sh
komorebic static-config-schema
komorebic application-specific-configuration-schema
komorebic notification-schema
komorebic socket-schema
```

[`notification-schema`](../cli/notification-schema.md) is the schema of what a
[subscriber](subscribing-to-notifications.md) is sent, and
[`socket-schema`](../cli/socket-schema.md) the schema of a message. To check a file by hand:

```sh
komorebic static-config-schema > schema.json
check-jsonschema --schemafile schema.json ~/.config/komorebi/komorebi.json
```

`check-jsonschema` reads plain JSON, so comments in the file have to come out first. The schema
does not stop komorebi from reading a file that does not follow it: a key it does not have is
dropped, as [above](#one-file-for-every-platform). [`komorebic check`](checking-the-setup.md)
reads the file the way the daemon does, and names a key that is not a setting, with the nearest
one that is.

