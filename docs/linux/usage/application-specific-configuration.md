# Application specific configuration

Some applications need to be told how to behave under a tiling window manager: a calculator that
should float instead of taking a tile, a launcher that should never be tiled. The
[rules](rules.md) that say so can be put in `komorebi.json`, or in a file of their own,
`applications.json`, that the configuration names. The file can be shared, or kept apart from
the settings, and a list of files can be put together from several sources.

## The file

`applications.json` is an object with one entry for each application. The name of the entry is
only for the reader. Each entry has lists of [rules](rules.md), as `komorebi.json` has them:

```json
{
  "KDE Plasma": {
    "floating": [
      { "kind": "Class", "id": "org.kde.kcalc", "matching_strategy": "Equals" },
      { "kind": "Class", "id": "org.kde.spectacle", "matching_strategy": "Equals" }
    ]
  },
  "Steam": {
    "ignore": [{ "kind": "Exe", "id": "steam", "matching_strategy": "Equals" }]
  }
}
```

| List | What it does | In `komorebi.json` |
|---|---|---|
| `ignore` | komorebi leaves the windows alone | `ignore_rules` |
| `manage` | komorebi tiles the windows, whatever the ignore rules say | `manage_rules` |
| `floating` | the windows open floating | `floating_applications` |
| `titleless`, `tabbed` | nothing: they work around how another platform exposes windows, and a KWin window is always a window | `titleless_applications`, `tabbed_applications` |

An entry named `$schema` has the address of a schema as its value, as an editor wants it, and
is ignored.

## Naming the file

`app_specific_configuration_path` in `komorebi.json` is the path of one file, or a list of paths:

```json
{
  "app_specific_configuration_path": "$HOME/.config/komorebi/applications.json"
}
```

```json
{
  "app_specific_configuration_path": [
    "$HOME/.config/komorebi/applications.json",
    "$HOME/.config/komorebi/mine.json"
  ]
}
```

Environment variables in a path, such as `$HOME`, are replaced. The files are read when
`komorebi.json` is: when the daemon starts, and again when the configuration changes. Their rules
are added to the ones the daemon has, after the rules of `komorebi.json` itself, which
[Configuration](configuration.md#rules) explains.

A path with no file behind it is skipped, and the daemon logs a warning: one `komorebi.json` can
serve every platform, even if it names a file that only another platform has. A file that is
there and is not an `applications.json`, or has a rule that is not valid, is an error: the daemon
does not start with it, and a reload with it fails and logs the error. Only a file with the
extension `.json` is read.

## The file komorebi ships

[`komorebic/applications.json`](../../../komorebic/applications.json) in the repository has an
entry for KDE Plasma, which floats the calculators of KDE and GNOME, the screenshot tool, the
authentication dialog of Plasma and System Settings. A test loads it, and opens the windows it
names.
