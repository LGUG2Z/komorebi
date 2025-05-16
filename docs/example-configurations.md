`komorebi`, and tiling window managers in general, are very complex pieces of
software.

In an attempt to reduce some of the initial configuration burden for users who
are looking to try out the software for the first time, example configurations
are provided and updated whenever appropriate.

## Downloading example configurations

Run the following command to download example configuration files for
`komorebi` and `whkd`. Pay attention to the output of the command to see where
the example files have been downloaded. For most new users this will be in the
`$Env:USERPROFILE` directory.

```powershell
komorebic quickstart
```

With the example configurations downloaded, you can now start `komorebi`,
`komorebi-bar` and `whkd`.

```powershell
komorebic start --whkd --bar
```

## komorebi.json

The example window manager configuration sets some sane defaults and provides
seven preconfigured workspaces on the primary monitor each with a different
layout.

```json
{% include "./komorebi.example.json" %}
```

### Application-specific configuration

There is a [community-maintained
repository](https://github.com/LGUG2Z/komorebi-application-specific-configuration)
of "apps behaving badly" that do not conform to Windows application development
guidelines and behave erratically when used with `komorebi` without additional
configuration.

You can always download the latest version of these configurations by running
`komorebic fetch-asc`. The output of this command will also provide a line that
you can paste into `komorebi.json` to ensure that the window manager looks for
the file in the correction location.

When installing and running `komorebi` for the first time, the `komorebic
quickstart` command will usually download this file to the `$Env:USERPROFILE`
directory.

### Padding

While you can set the workspace padding (the space between the outer edges of
the windows and the bezel of your monitor) and the container padding (the space
between each of the tiled windows) for each workspace independently, you can
also set a default for both of these values that will apply to all workspaces
using `default_workspace_padding` and `default_container_padding`.

### Active window border

You may have seen videos and screenshots of people using `komorebi` with a
thick, colourful active window border. You can also enable this by setting
`border` to `true`. However, please be warned that this feature
is a crude hack trying to compensate for the insistence of Microsoft Windows
design teams to make custom borders with widths that are actually visible to
the user a thing of the past and removing this capability from the Win32 API.

I know it's buggy, and I know that most of the it sucks, but this is something
you should be bring up with the billion dollar company and not with me, the
solo developer.

### Border colours

If you choose to use the active window border, you can set different colours to
give you visual queues when you are focused on a single window, a stack of
windows, or a window that is in monocle mode.

The example colours given are blue single, green for stack and pink for
monocle.

### Layouts

#### BSP

```
+-------+-----+
|       |     |
|       +--+--+
|       |  |--|
+-------+--+--+
```

#### Vertical Stack

```
+-------+-----+
|       |     |
|       +-----+
|       |     |
+-------+-----+
```

#### RightMainVerticalStack

```
+-----+-------+
|     |       |
+-----+       |
|     |       |
+-----+-------+
```

#### Horizontal Stack

```
+------+------+
|             |
|------+------+
|      |      |
+------+------+
```

#### Columns

```
+--+--+--+--+
|  |  |  |  |
|  |  |  |  |
|  |  |  |  |
+--+--+--+--+
```

#### Rows

If you have a vertical monitor, I recommend using this layout.

```
+-----------+
|-----------|
|-----------|
|-----------|
+-----------+
```

#### Ultrawide Vertical Stack

If you have an ultrawide monitor, I recommend using this layout.

```
+-----+-----------+-----+
|     |           |     |
|     |           +-----+
|     |           |     |
|     |           +-----+
|     |           |     |
+-----+-----------+-----+
```

### Grid

If you like the `grid` layout in [LeftWM](https://github.com/leftwm/leftwm-layouts) this is almost exactly the same!

The `grid` layout does not support resizing windows tiles.

```
+-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
|     |     |   |   |   |   |   |   |   |   |   |   |   |   |
|     |     |   |   |   |   |   |   |   |   |   |   |   +---+
+-----+-----+   |   +---+---+   +---+---+---+   +---+---|   |
|     |     |   |   |   |   |   |   |   |   |   |   |   +---+
|     |     |   |   |   |   |   |   |   |   |   |   |   |   |
+-----+-----+   +---+---+---+   +---+---+---+   +---+---+---+
  4 windows       5 windows       6 windows       7 windows
```

## whkdrc

`whkd` is a fairly basic piece of software with a simple configuration format:
key bindings go to the left of the colon, and shell commands go to the right of the
colon.

As of [`v0.2.4`](https://github.com/LGUG2Z/whkd/releases/tag/v0.2.4), `whkd` can override most of Microsoft's
limitations on hotkey bindings that include the `win` key. However, you will still need
to [modify the registry](https://superuser.com/questions/1059511/how-to-disable-winl-in-windows-10) to prevent
`win + l` from locking the operating system.

You can toggle an overlay of the current `whkdrc` shortcuts related to `komorebi` at any time when using the example
configuration with `alt + i`.

```
{% include "./whkdrc.sample" %}
```

### Configuration

`whkd` searches for a `whkdrc` configuration file in the following locations:

* `$Env:WHKD_CONFIG_HOME`
* `$Env:USERPROFILE/.config`

It is also possible to change a hotkey behavior depending on which application has focus:

```
alt + n [
    # ProcessName as shown by `Get-Process`
    Firefox       : echo "hello firefox"

    # Spaces are fine, no quotes required
    Google Chrome : echo "hello chrome"
]
```

### Setting .shell

There is one special directive at the top of the file, `.shell` which can be
set to either `powershell`, `pwsh` or `cmd`. Which one you use will depend on
which shell you use in your terminal.

* `powershell` - set this if you are using the version of PowerShell that comes
  installed with Windows 10+ (the executable file for this is `powershell.exe`)

* `pwsh` - set this if you are using PowerShell 7+, which you have installed yourself either through the Windows Store
  or WinGet (the executable file for this is `pwsh.exe`)

* `cmd` - set this if you don't want to use PowerShell at all and instead you
  want to call commands through the shell used by the old-school Command
  Prompt (the executable file for this is `cmd.exe`)

### Key codes

Key codes for alphanumeric and arrow keys are just what you would expect. For
punctuation and other keys, please refer to the [Virtual Key
Codes](https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes)
reference.

If you want to use one of those key codes, put them into lower case and remove
the `VK_` prefix. For example, the keycode `VK_OEM_PLUS` becomes `oem_plus` in
the sample configuration above.

## komorebi.bar.json

The example status bar configuration sets some sane defaults and provides
a number of pre-configured widgets on the primary monitor.

```json
{% include "./komorebi.bar.example.json" %}
```

### Themes

Themes can be set in either `komorebi.json` or `komorebi.bar.json`. If set
in `komorebi.json`, the theme will be applied to both komorebi's borders and
stackbars as well as the status bar.

If set in `komorebi.bar.json`, the theme will only be applied to the status bar.

All [Catppuccin palette variants](https://catppuccin.com/)
and [most Base16 palette variants](https://tinted-theming.github.io/tinted-gallery/)
are available as themes.
