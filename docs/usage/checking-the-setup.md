# Checking the setup

[`komorebic check`](../cli/check.md) reads your configuration the way the daemon does, and says in
words what is wrong with it. It does not need the daemon to be running, and it changes nothing.

```sh
komorebic check
komorebic check --komorebi-config ~/mine.json
```

It reads `komorebi.json` in the [config home](configuration.md#where-the-file-is), or the file
`--komorebi-config` names (`-k`; environment variables in the path are replaced). It prints a
section of findings, then the number of errors and warnings, and exits with status 1 if and only if
a finding is an error, so a script can run it before it starts the daemon.

## What a finding says

Each finding is on a line of its own, after its level, with advice under it when there is some:

| Level | Meaning |
|---|---|
| `ok` | Right as it is |
| `note` | Something to know |
| `warning` | Something that will not do what was meant: komorebi starts and carries on |
| `error` | Something that stops komorebi, or a part of it, from working |

## The configuration

The first section says where komorebi looks, and then what it finds in the file:

- **No file.** A `note`: komorebi starts with its defaults, and
  [`komorebic quickstart`](quickstart-and-autostart.md#quickstart) has an example to begin from.
- **JSON that does not parse.** An `error` that shows the line, and the two before it, with a caret
  under the place serde stopped:

  ```text
  error   bad.json is not valid JSON: expected ident at line 3 column 29
          1 | {
          2 |   "resize_delta": 100,
          3 |   "mouse_follows_focus": tru,
            |                             ^
  ```

- **A value komorebi does not read.** An `error` that shows the line the same way, for a value of
  the wrong kind or a name that is not one of the choices (`"cross_boundary_behaviour":
  "Sideways"`). The hint says that [`komorebic static-config-schema`](../cli/static-config-schema.md)
  prints the schema, for an editor to check the file against.
- **A key that is not a setting.** A `warning`, with the nearest setting when one is near enough to
  be what you meant, at the root of the file, in a monitor and in a workspace. komorebi reads the
  file without it, so a typo is ignored, which is why the check says so:

  ```text
  warning `resize_dlta` is not a setting komorebi has
          did you mean `resize_delta`?
  ```

- **A key for another platform.** Not a mistake, since one `komorebi.json` can serve
  [every platform](configuration.md#one-file-for-every-platform). The keys that only komorebi for
  Windows or for macOS reads, and the keys komorebi for KDE reads and does nothing with, are listed
  once, in a `note`. A workspace's `custom_layout` and `custom_layout_rules`, which Windows has, are
  a `warning`: they are not supported yet.
- **An application specific configuration that is not there.** A `warning` for each file that
  `app_specific_configuration_path` names and that does not exist: komorebi starts without it,
  and the windows it names are managed like any other.
- **A wallpaper that is not a file.** An `error` for each wallpaper, of a monitor or of a
  workspace, whose `path` is not a file: the shell has nothing to show.
- **A wallpaper no theme can be made from.** A `warning` for a picture of which the
  [borders cannot take their colors](wallpapers.md#the-colors-of-the-borders), when
  `generate_theme` is not `false`: the wallpaper is set and the borders keep the colors they have.
- **A workspace that is only a name.** A `warning` for a workspace with no `layout`, no `tile` and
  no `layout_rules`, which komorebi does not tile.
- **Placement rules that are not used.** A `note` for a workspace that has
  [`initial_window_placement_rules`](../initial-window-placement-rules.md) and
  [appends windows to a container](stacking-windows.md) (`"window_container_behaviour":
  "Append"`, on the workspace, or in the file when the workspace does not say): a window that is
  appended has no place of its own to be given.

The check takes the keys it knows from the schema of the types the daemon reads the file with, at
every level, so it knows every setting komorebi reads.
