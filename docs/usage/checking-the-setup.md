# Checking the setup

[`komorebic check`](../cli/check.md) reads your configuration the way the daemon does, looks at the
desktop komorebi has to run on, and says in words what is wrong. It does not need the daemon to be
running, and it changes nothing.

```sh
komorebic check
komorebic check --komorebi-config ~/mine.json
komorebic check --configuration-only
```

It reads `komorebi.json` in the [config home](configuration.md#where-the-file-is), or the file
`--komorebi-config` names (`-k`; environment variables in the path are replaced). It prints a
section of findings for the configuration and one for the desktop, then the number of errors and
warnings, and exits with status 1 if and only if a finding is an error, so a script can run it
before it starts the daemon. `--configuration-only` leaves the desktop out, for a machine that is
not the one komorebi will run on: a build that checks a file, say.

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

## The desktop

The second section is about the desktop `komorebic check` runs in, which has to be the session
komorebi runs in: it asks the session bus of the environment it is run from, so run it in a
terminal of the Plasma session, not over ssh. It looks, in this order, at:

- **The session.** A `warning` when `XDG_CURRENT_DESKTOP` does not name KDE, and one when the
  session is not Wayland (`XDG_SESSION_TYPE` is not `wayland` and there is no `WAYLAND_DISPLAY`):
  komorebi is made for KWin on Wayland and tried on nothing else.
- **The session bus, and KWin on it.** An `error` when it cannot connect to the session bus (the
  hint says that `DBUS_SESSION_BUS_ADDRESS` has to be that of the desktop session, which it is not
  over ssh), when the bus does not answer, and when KWin is not on it, since komorebi works through
  KWin and nothing else. The check goes no further after one of them, because nothing after it can
  be answered.
- **KWin's version.** `ok` for KWin 6 or newer, an `error` for an older one, and a `note` when
  KWin does not say what version it is.
- **komorebi and its script.** komorebi is running when its socket takes a connection, and its
  script is in KWin when KWin says it has loaded `komorebi-bridge`:

  | komorebi | script | Finding |
  |---|---|---|
  | running | in KWin | `ok` |
  | running | not in KWin | `error`: it cannot see a window; `komorebic log` says why, and restarting komorebi loads the script again |
  | not running | in KWin | `warning`: a script of a daemon that is gone is still in KWin, and starting komorebi replaces it |
  | not running | not in KWin | `note`: `komorebic start` |

- **The border effect.** The borders are drawn by an effect of KWin, KDE Rounded Corners, and
  only komorebi's build of it has a color for each kind of window (the stock one has a color for the
  active window and one for the others). `ok` when the effect is komorebi's build; a `warning` when
  `border` is on and the effect is not installed, or is the stock one, and a `note` when `border` is
  off. The build is told by what it contains (the name of its D-Bus interface in the plugin file,
  or its object on the bus), not by its name.
- **The animation effect.** Windows are animated by Geometry Change, a script effect, and only
  komorebi's build reads its curve from a setting (the stock one eases out exponentially and
  nothing else). `ok` when the copy KWin uses is komorebi's build. A `warning` when `animation` is
  on and the effect is not installed, and when it is the stock one and `animation.style` is any
  other than `EaseOutExpo` (the style is ignored, and the default is `Linear`); a `note` when
  animation is off. A `warning` too when a copy in your own data directory, which KWin takes before
  the system's, is not the same build as the system's and hides it.
- **The Plasma shell.** Only when a monitor or a workspace has a
  [wallpaper](wallpapers.md): `ok` when the shell is on the bus, a `warning` when it is not
  (wallpapers will not be set; komorebi asks again for half a minute after it starts, for a shell
  that is late).
- **What starts komorebi at login.** `ok` with the file that does it: the autostart entry
  [`enable-autostart`](quickstart-and-autostart.md#starting-komorebi-with-plasma) writes, or a
  systemd user unit (`~/.config/systemd/user`, `/etc/systemd/user`, or the one of the NixOS module).
  A `note` when there is none. It is looked for after the rest, whatever the rest found.

Effects are looked for where KWin looks: the check reads the environment of the process that owns
`org.kde.KWin` (`/proc/PID/environ`) for the plugin and data directories, so the answer is KWin's
and not that of the shell the command is typed in.

Every question to the bus has two seconds to be answered, so a bus that never answers is an `error`
like one that is not there.
