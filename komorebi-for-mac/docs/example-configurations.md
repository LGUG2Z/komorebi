`komorebi`, and tiling window managers in general, are very complex pieces of
software.

In an attempt to reduce some of the initial configuration burden for users who
are looking to try out the software for the first time, example configurations
are provided and updated whenever appropriate.

## Downloading example configurations

Run the following command to download example configuration files for
`komorebi` and `skhd`. Pay attention to the output of the command to see where
the example files have been downloaded. For most new users this will be in the
`$HOME/.config/komorebi` directory.

```bash
komorebic quickstart
```

### Granting Permissions

The `quickstart` command will also prompt you to grant komorebi and the terminal
emulator you are start komorebi with the permissions it requires to run, and
will open the following System Settings tabs for you:

* Settings -> Privacy & Security -> Accessibility
* Settings -> Privacy & Security -> Screen & System Audio Recording

### Corporate Devices Enrolled in MDM

If you are using `komorebi` on a corporate device enrolled in mobile device
management, you will receive a pop-up when you run `komorebic start` reminding
you that the [Komorebi License](https://github.com/LGUG2Z/komorebi-license) does
not permit any kind of commercial use.

You can remove this pop-up by running `komorebic license <email>` with the email
associated with your Individual Commercial Use License. A single HTTP request
will be sent with the given email address to verify license validity.

## Starting komorebi

With the example configurations downloaded and permissions granted, you can now
start `komorebi`:

```powershell
komorebic start
```

Alternatively, if you also want to start with the status bar, you can run:

```powershell
komorebic start --bar
```

Before running the status bar, it is recommended to set the system's status bar
to autohide.

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
of "apps behaving badly" that do not conform to macOS application development
guidelines and behave erratically when used with `komorebi` without additional
configuration.

You can always download the latest version of these configurations by running
`komorebic fetch-asc`. The output of this command will also provide a line that
you can paste into `komorebi.json` to ensure that the window manager looks for
the file in the correction location.

When installing and running `komorebi` for the first time, the `komorebic
quickstart` command will usually download this file to the `$HOME/.config/komorebi`
directory.

### Padding

While you can set the workspace padding (the space between the outer edges of
the windows and the bezel of your monitor) and the container padding (the space
between each of the tiled windows) for each workspace independently, you can
also set a default for both of these values that will apply to all workspaces
using `default_workspace_padding` and `default_container_padding`.

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

## skhdrc

`skhd` is a fairly basic piece of software with a simple configuration format:
key bindings go to the left of the colon, and shell commands go to the right of the
colon.

```
{% include "./skhdrc.sample" %}
```

You can try the example komorebi configuration by running
`skhd --config ~/.config/komorebi/skhdrc`.

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