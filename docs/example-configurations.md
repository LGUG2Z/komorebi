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

With the example configurations downloaded, you can now start `komorebi` and `whkd.

```powershell
komorebic start --whkd
```

## komorebi.json

The example window manager configuration sets some sane defaults and provides
five preconfigured workspaces on the primary monitor each with a different
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
`active_window_border` to `true`. However, please be warned that this feature
is a crude hack trying to compensate for the insistence of Microsoft Windows
design teams to make custom borders with widths that are actually visible to
the user a thing of the past and removing this capability from the Win32 API.

I know it's buggy, and I know that most of the it sucks, but this is something
you should be bring up with the billion dollar company and not with me, the
solo developer.

### Border colours

If you choose to use the active window border, you can set different colours to
give you visual queues when you are focused on a single window, a stack of
windows, or a window that is in monocole mode.

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
key bindings go to the left of the, and shell commands go to the right of the
colon.

Please remember that `whkd` does not support overriding Microsoft's limitations
on hotkey bindings that include the `Windows` key. If this is important to you,
I recommend using [AutoHotKey](https://autohotkey.com) to set up your key
bindings for `komorebic` commands instead.

```
{% include "./whkdrc.sample" %}
```

### Setting .shell

There is one special directive at the top of the file, `.shell` which can be
set to either `powershell`, `pwsh` or `cmd`. Which one you use will depend on
which shell you use in your terminal.

* `powershell` - set this if you are using the version of PowerShell that comes
  installed with Windows 10+ (the executable file for this is `powershell.exe`)

* `pwsh` - set this if you are using PowerShell 7+, which you have installed yourself either through the Windows Store or WinGet (the executable file for this is `pwsh.exe`)

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
