# komorebi

Tiling Window Management for Windows.

<p>
  <img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/LGUG2Z/komorebi/.github/workflows/windows.yaml">
  <img alt="GitHub" src="https://img.shields.io/github/license/LGUG2Z/komorebi">
  <img alt="GitHub all releases" src="https://img.shields.io/github/downloads/LGUG2Z/komorebi/total">
  <img alt="GitHub commits since latest release (by date) for a branch" src="https://img.shields.io/github/commits-since/LGUG2Z/komorebi/latest">
  <a href="https://discord.gg/mGkn66PHkx">
    <img alt="Discord" src="https://img.shields.io/discord/898554690126630914">
  </a>
  <a href="https://github.com/sponsors/LGUG2Z">
    <img alt="GitHub Sponsors" src="https://img.shields.io/github/sponsors/LGUG2Z">
  </a>
  <a href="https://notado.app/feeds/jado/software-development">
    <img alt="Notado Feed" src="https://img.shields.io/badge/Notado-Subscribe-informational">
  </a>
  <a href="https://jeezy.substack.com">
    <img alt="Substack Read" src="https://img.shields.io/badge/Substack-Read-orange">
  </a>
  <a href="https://twitter.com/LGUG2Z">
    <img alt="Twitter Follow" src="https://img.shields.io/twitter/follow/LGUG2Z">
  </a>
</p>

![screenshot](https://user-images.githubusercontent.com/13164844/184027064-f5a6cec2-2865-4d65-a549-a1f1da589abf.png)

- [About](#about)
- [Charitable Donations](#charitable-donations)
- [GitHub Sponsors](#github-sponsors)
- [Demonstrations](#demonstrations)
- [Description](#description)
- [Design](#design)
- [Getting Started](#getting-started)
  - [Quickstart](#quickstart)
  - [GitHub Releases](#github-releases)
  - [Building from Source](#building-from-source)
  - [Running](#running)
  - [Configuring](#configuring)
  - [Common First-Time Tips](#common-first-time-tips)
- [Development](#development)
- [Logs and Debugging](#logs-and-debugging)
  - [Restoring Windows](#restoring-windows)
  - [Panics and Deadlocks](#panics-and-deadlocks)
- [Window Manager State and Integrations](#window-manager-state-and-integrations)
- [Window Manager Event Subscriptions](#window-manager-event-subscriptions)
  - [Subscription Event Notification Schema](#subscription-event-notification-schema)
  - [Communication over TCP](#communication-over-tcp)
  - [Socket Message Schema](#socket-message-schema)
- [Appreciations](#appreciations)

## About

_komorebi_ is a tiling window manager that works as an extension to
Microsoft's [Desktop Window Manager](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview) in Windows 10 and
above.

_komorebi_ allows you to control application windows, virtual workspaces and display monitors with a CLI which can be
used with third-party software such as [AutoHotKey](https://github.com/Lexikos/AutoHotkey_L) to set user-defined
keyboard shortcuts.

_komorebi_ aims to make _as few modifications as possible_ to the operating system and desktop environment by default.
Users are free to make such modifications in their own configuration files for _komorebi_, but these will remain
opt-in and off-by-default for the foreseeable future.

Translations of this document can be found in the project wiki:

- [komorebi 中文用户指南](https://github.com/LGUG2Z/komorebi/wiki/README-zh) (by [@crosstyan](https://github.com/crosstyan))

There is a [Discord server](https://discord.gg/mGkn66PHkx) available for _komorebi_-related discussion, help,
troubleshooting etc. If you have any specific feature requests or bugs to report, please create an issue in this
repository.

There is a [YouTube channel](https://www.youtube.com/channel/UCeai3-do-9O4MNy9_xjO6mg) where I livestream development
on _komorebi_. If you would like to be notified of upcoming livestreams please subscribe and turn on
notifications. Videos of previous livestreams are also made available in
a [dedicated playlist](https://www.youtube.com/playlist?list=PLllZnrEJu89Cpu4tMO8LAg1m6gWYWLSGJ).

Articles, blog posts, demos, and videos about _komorebi_ can be added to this list by PR:

- [Moving to Windows from Linux Pt 1](https://kvwu.io/posts/moving-to-windows/)
- [Windows 下的现代化平铺窗口管理器 komorebi](https://zhuanlan.zhihu.com/p/455064481)
- [komorebi を導入してみる](https://zenn.dev/omochice/articles/50f42a3df8f426)

## Charitable Donations

_komorebi_ is a free and open-source project, and one that encourages you to make charitable donations if
you find the software to be useful and have the financial means.

I encourage you to make a charitable donation
to [Fresh Start Refugee](https://www.freshstartrefugee.org/donate) before
you consider sponsoring me on GitHub.

## GitHub Sponsors

[GitHub Sponsors is enabled for this project](https://github.com/sponsors/LGUG2Z). Users who sponsor my work
on `komorebi` at any of the predefined monthly tiers will be given access to a private fork of this repository where I
push features-in-progress that are not yet quite ready to be pushed on the main repository.

There will never be any feature of `komorebi` that is gated behind sponsorship; every new feature will always be
available for free in the public repository once it meets the requisite level of code quality and completion.

Features-in-progress that are available in early access will be tagged in the issues with
an ["early access" label](https://github.com/LGUG2Z/komorebi/issues?q=is%3Aopen+is%3Aissue+label%3A%22early+access%22).

## Demonstrations

[@haxibami](https://github.com/haxibami) showing _komorebi_ running on Windows
11 with a terminal emulator, a web browser and a code editor. The original
video can be viewed
[here](https://twitter.com/haxibami/status/1501560766578659332).

https://user-images.githubusercontent.com/13164844/163496447-20c3ff0a-c5d8-40d1-9cc8-156c4cebf12e.mp4

[@aik2mlj](https://github.com/aik2mlj) showing _komorebi_ running on Windows 11
with multiple workspaces, terminal emulators, a web browser, and the
[yasb](https://github.com/DenBot/yasb) status bar with the _komorebi_ workspace
widget enabled. The original video can be viewed
[here](https://zhuanlan.zhihu.com/p/455064481).

https://user-images.githubusercontent.com/13164844/163496414-a9cde3d1-b8a7-4a7a-96fb-a8985380bc70.mp4

## Description

_komorebi_ only responds to [WinEvents](https://docs.microsoft.com/en-us/windows/win32/winauto/event-constants) and the
messages it receives on a dedicated socket.

_komorebic_ is a CLI that writes messages on _komorebi_'s socket.

_komorebi_ doesn't handle any keyboard or mouse inputs; a third party program (e.g.
[whkd](https://github.com/LGUG2Z/whkd)) is needed in order to translate keyboard and mouse events to _komorebic_ commands.

This architecture, popularised by [_bspwm_](https://github.com/baskerville/bspwm) on Linux and
[_yabai_](https://github.com/koekeishiya/yabai) on macOS, is outlined as follows:

```
          PROCESS                SOCKET
whkd/ahk  -------->  komorebic  <------>  komorebi
```

## Design

_komorebi_ holds a list of physical monitors.

A monitor is just a rectangle of the available work area which contains one or more virtual workspaces.

A workspace holds a list of containers.

A container is just a rectangle where one or more application windows can be displayed.

This means that:

- Every monitor has its own collection of virtual workspaces
- Workspaces only know about containers and their dimensions, not about individual application windows
- Every application window must belong to a container, even if that container only contains one application window
- Many application windows can be stacked and cycled through in the same container within a workspace

## Getting Started

### Quickstart

Make sure that you have either the [Scoop Package Manager](https://scoop.sh) or WinGet installed, then run the following
commands at a PowerShell prompt.

```powershell
# if using scoop
scoop bucket add extras
scoop install whkd
scoop install komorebi

# if using winget
winget install LGUG2Z.whkd
winget install LGUG2Z.komorebi

# save the latest generated app-specific config tweaks and fixes to ~/komorebi.generated.ps1
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/komorebi.generated.ps1 -OutFile $Env:USERPROFILE\komorebi.generated.ps1

# save the sample komorebi configuration file to ~/komorebi.ps1
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/komorebi.sample.ps1 -OutFile $Env:USERPROFILE\komorebi.ps1

# ensure the ~/.config folder exists
mkdir $Env:USERPROFILE\.config -ea 0

# save the sample whkdrc file with key bindings to ~/.config/whkdrc
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/whkdrc.sample -OutFile $Env:USERPROFILE\.config\whkdrc

# start komorebi
komorebic start --await-configuration
```

Thanks to [@sitiom](https://github.com/sitiom) for getting _komorebi_ added to both the popular Scoop Extras bucket and
to WinGet.

You can watch a walkthrough video of this quickstart below on YouTube.

[![Watch the quickstart walkthrough video](https://img.youtube.com/vi/cBnLIwMtv8g/hqdefault.jpg)](https://www.youtube.com/watch?v=cBnLIwMtv8g)

#### Using Autohotkey

If you would like to use Autohotkey, please make sure you have AutoHotKey v2 installed.

Generally, users who opt for AHK will have specific needs that can only be addressed by the advanced functionality of AHK,
and so they are assumed to be able to craft their own configuration files.

If you would like to try out AHK, a simple sample configuration powered by `komorebic.lib.ahk` is provided as a starting
point.

```powershell
# save the latest generated komorebic library to ~/komorebic.lib.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/komorebic.lib.ahk -OutFile $Env:USERPROFILE\komorebic.lib.ahk

# save the latest generated app-specific config tweaks and fixes to ~/komorebi.generated.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/komorebi.generated.ahk -OutFile $Env:USERPROFILE\komorebi.generated.ahk

# save the sample komorebi configuration file to ~/komorebi.ahk
iwr https://raw.githubusercontent.com/LGUG2Z/komorebi/master/komorebi.sample.ahk -OutFile $Env:USERPROFILE\komorebi.ahk
```

### GitHub Releases

Prebuilt binaries are available on the [releases page](https://github.com/LGUG2Z/komorebi/releases) in a `zip` archive.
Once downloaded, you will need to move the `komorebi.exe` and `komorebic.exe` binaries to a directory in your `Path` (
you can see these directories by running `$Env:Path.split(";")` at a PowerShell prompt).

### Building from Source

If you prefer to compile _komorebi_ from source, you will need
a [working Rust development environment on Windows 10/11](https://rustup.rs/). The `x86_64-pc-windows-msvc` toolchain is
required, so make sure you have also installed
the [Build Tools for Visual Studio 2019](https://stackoverflow.com/a/55603112).

You can then clone this repo and compile the source code to install the binaries for `komorebi` and `komorebic`:

```powershell
cargo install --path komorebi --locked
cargo install --path komorebic --locked
```

### Running

Run `komorebic start --await-configuration` at a Powershell prompt, and you will see the following output:

```
Start-Process komorebi.exe -ArgumentList '--await-configuration' -WindowStyle hidden
Waiting for komorebi.exe to start...Started!
```

This means that `komorebi` is now running in the background, tiling all your windows, and listening for commands sent to
it by `komorebic`. You can similarly stop the process by running `komorebic stop`.

### Configuring

If you followed the quickstart, `komorebi` will find the sample `komorebi.ps1` file in your `$Env:USERPROFILE` directory
and automatically load it. This file also starts `whkd` using the sample `whkrc` file in your `$Env:USERPROFILE\.config`
directory.

Alternatively, if you have AutoHotKey installed and a `komorebi.ahk` file in `$Env:UserProfile` directory, `komorebi`
will automatically try to load it when starting.

#### Configuration with `komorebic`

As previously mentioned, this project does not handle anything related to keybindings and shortcuts directly. I
personally use [`whkd`](https://github.com/LGUG2Z/whkd) to manage my window management shortcuts, and have provided a
sample [whkdrc](whkdrc.sample) configuration that you can use as a starting point for your own.

You can run `komorebic.exe` to get a full list of the commands that you can use to customise `komorebi` and create
keybindings with. You can run `komorebic.exe <COMMAND> --help` to get a full explanation of the arguments required for
each command.

You can run any configuration command in the `komorebi.ps1` file, and you can bind any action command to your desired
key combinations in the `whkdrc` file.

#### AutoHotKey Helper Library for `komorebic`

❗️**NOTE**: This section is only relevant for people who wish to use AutoHotKey instead of [`whkd`](https://github.com/LGUG2Z/whkd).

❗️**NOTE**: This helper library is only compatible with AutoHotKey v1.1, not with AutoHotKey v2.

Additionally, you may run `komorebic.exe ahk-library` to generate a helper library for AutoHotKey which wraps
every `komorebic` command in a native AHK function.

The output of this command is in AHKv1 syntax. It must be manually converted to AHKv2 syntax
using [this tool](https://github.com/mmikeww/AHK-v2-script-converter) or something similar.

If you include the generated library at the top of your `~/komorebi.ahk` configuration file, you will be able to call
any of the functions that it contains.

#### Using Different AHK Executables

❗️**NOTE**: This section is only relevant for people who wish to use AutoHotKey instead of [`whkd`](https://github.com/LGUG2Z/whkd).

The preferred way to install AutoHotKey for use with `komorebi` is to install it via `scoop`:

```powershell
scoop bucket add versions
scoop install autohotkey
```

If you install AutoHotKey using a different method, the name of the executable file may differ from the name given by
`scoop`, and thus what is expected by default in `komorebi`.

You may override the executable that `komorebi` looks for to launch and reload `komorebi.ahk` configuration files by
setting the `$Env:KOMOREBI_AHK_EXE` environment variable.

Please keep in mind that even when setting a custom executable name using these environment variables, the executables
are still required to be in your `Path`.

### Common First-Time Tips

#### Setting a Custom KOMOREBI_CONFIG_HOME Directory

If you do not want to keep _komorebi_-related files in your `$Env:USERPROFILE` directory, you can specify a custom directory
by setting the `$Env:KOMOREBI_CONFIG_HOME` environment variable.

For example, to use the `~/.config/komorebi` directory:

```powershell
# Run this command to make sure that the directory has been created
mkdir -p ~/.config/komorebi

# Run this command to open up your PowerShell profile configuration in Notepad
notepad $PROFILE

# Add this line (with your login user!) to the bottom of your PowerShell profile configuration
$Env:KOMOREBI_CONFIG_HOME = 'C:\Users\LGUG2Z\.config\komorebi'

# Save the changes and then reload the PowerShell profile
. $PROFILE
```

If you already have configuration files that you wish to keep, move them to the `~/.config/komorebi` directory.

The next time you run `komorebic start`, any files created by or loaded by _komorebi_ will be placed or expected to
exist in this folder.

#### Generating Common Application-Specific Configurations

A curated selection of application-specific configurations can be generated to
help ease the setup for first-time users.
[`komorebi-application-specific-configuration`](https://github.com/LGUG2Z/komorebi-application-specific-configuration)
contains YAML definitions of settings that are known to make tricky
applications behave as expected. These YAML definitions can be used to generate
a `ps1` or an `ahk` file which you can import at the start of your own `komorebi.ps1` or `komorebi.ahk` files,
leaving you to focus primarily on your desired keybindings and workspace
configurations.

If you have settings for an application that you think should be part of this
curated selection, please open a PR on the configuration repository.

In the event that your PR is not accepted, or if you find there are any
settings that you wish to override, this can easily be done using an override
file.

```powershell
# Clone and enter the repository
git clone https://github.com/LGUG2Z/komorebi-application-specific-configuration.git
cd komorebi-application-specific-configuration

# Use komorebic to generate a ps1 file
komorebic.exe pwsh-app-specific-configuration applications.yaml

# Application-specific generated configuration written to C:\Users\LGUG2Z\.config\komorebi\komorebi.generated.ps1

# Or use komorebic to generate an ahk file
komorebic.exe ahk-app-specific-configuration applications.yaml

# Application-specific generated configuration written to C:\Users\LGUG2Z\.config\komorebi\komorebi.generated.ahk
#
# You can include the generated configuration at the top of your komorebi.ahk config with this line:
#
# #Include %A_ScriptDir%\komorebi.generated.ahk

# Optionally, provide an override file that follows the same schema as the second argument
komorebic.exe pwsh-app-specific-configuration applications.yaml overrides.yaml
```

#### Adding an Active Window Border

If you would like to add a visual border around the currently focused window, two commands are available:

```powershell
komorebic.exe active-window-border [enable|disable]
komorebic.exe active-window-border-colour [R G B] --window-kind single

# optionally, if you want a different colour for stacks of windows
komorebic.exe active-window-border-colour [R G B] --window-kind stack

# optionally, if you want a different colour for windows in monocle mode
komorebic.exe active-window-border-colour [R G B] --window-kind monocle
```

It is important to note that the active window border will only apply to windows managed by `komorebi`.

[![Watch the tutorial video](https://img.youtube.com/vi/ywiAvoMV_gE/hqdefault.jpg)](https://www.youtube.com/watch?v=ywiAvoMV_gE)

#### Removing Gaps

If you would like to remove all gaps from a given workspace, both between windows themselves, and between the monitor edges and the windows, you can set the following two configuration options to `0` for the desired monitors and workspaces:

```powershell
komorebic.exe container-padding <MONITOR_INDEX> <WORKSPACE_INDEX> 0
komorebic.exe workspace-padding <MONITOR_INDEX> <WORKSPACE_INDEX> 0
```

[![Watch the tutorial video](https://img.youtube.com/vi/eGr07mymgWE/hqdefault.jpg)](https://www.youtube.com/watch?v=eGr07mymgWE)

#### Multiple Layout Changes on Startup

❗️**NOTE**: If you followed the quickstart and are using the sample configurations, this is already the default behaviour.

Depending on what is in your configuration, when `komorebi` is started, you may experience the layout rapidly being adjusted
with many retile events.

If you would like to avoid this, you can start `komorebi` with a flag which tells `komorebi` to wait until all configuration
has been loaded before listening to and responding to window manager events: `komorebic start --await-configuration`.

If you start `komorebi` with the `--await-configuration` flag, you _must_ send the `komorebic complete-configuration`
command at the end of the configuration section of your `komorebi.ps1` (or `komorebi.ahk` config, before you start
defining the key bindings). The layout will not be updated and `komorebi` will not respond to `komorebic` commands until
this command has been received.

#### Floating Windows

❗️**NOTE**: A significant number of floating window rules for the most common applications are
[already generated for you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

Sometimes you will want a specific application to never be tiled, and instead float all the time. You can add rules to
enforce this behaviour:

```powershell
komorebic.exe float-rule title "Control Panel"
# komorebic.exe float-rule exe [EXE NAME]
# komorebic.exe float-rule class [CLASS NAME]
```

#### Windows Not Getting Managed

❗️**NOTE**: A significant number of force-manage window rules for the most common applications are
[already generated for you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

In some rare cases, a window may not automatically be registered to be managed by `komorebi`. When this happens, you can
manually add a rule to force `komorebi` to manage it:

```powershell
komorebic.exe manage-rule exe TIM.exe
# komorebic.exe manage-rule class [CLASS NAME]
# komorebic.exe manage-rule title [TITLE]
```

#### Tray Applications

❗️**NOTE**: A significant number of tray application rules for the most common applications are
[already generated for you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

If you are experiencing behaviour where
[closing a window leaves a blank tile, but minimizing the same window does not](https://github.com/LGUG2Z/komorebi/issues/6)
, you have probably enabled a 'close/minimize to tray' option for that application. You can tell _komorebi_ to handle
this application appropriately by identifying it via the executable name or the window class:

```powershell
komorebic.exe identify-tray-application exe Discord.exe
# komorebic.exe identify-tray-application class [CLASS NAME]
# komorebic.exe identify-tray-application title [TITLE]
```

#### Microsoft Office Applications

❗️**NOTE**: Microsoft Office-specific application rules are
[already generated for you](https://github.com/LGUG2Z/komorebi/#generating-common-application-specific-configurations)

Microsoft Office applications such as Word and Excel require certain configuration options to be set in order to be
managed correctly. Below is an example of configuring Microsoft Word to be managed correctly by _komorebi_.

```powershell
# This only needs to be added once
komorebic.exe float-rule class _WwB

# Repeat these for other office applications such as EXCEL.EXE etc
# Note that the capitalised EXE is important here- double check the
# exact case for the name and the file extension in Task Manager or
# the AHK Window Spy

komorebic.exe identify-layered-application exe WINWORD.EXE
komorebic.exe identify-border-overflow-application exe WINWORD.EXE
```

#### Focus Follows Mouse

`komorebi` supports two focus-follows-mouse implementations; the native Windows Xmouse implementation, which treats the
desktop, the task bar, and the system tray as windows and switches focus to them eagerly, and a custom `komorebi`
implementation, which only considers windows managed by `komorebi` as valid targets to switch focus to when moving the
mouse.

To enable the `komorebi` implementation you must start the process with the `--ffm` flag to explicitly enable the feature.
This is because the mouse tracking required for this feature significantly increases the CPU usage of the process (on my
machine, it jumps from <1% to ~4~), and this CPU increase persists regardless of whether focus-follows-mouse is enabled
or disabled at any given time via `komorebic`'s configuration commands.

When calling any of the `komorebic` commands related to focus-follows-mouse functionality, the `windows`
implementation will be chosen as the default implementation. You can optionally specify the `komorebi` implementation by
passing it as an argument to the `--implementation` flag:

```powershell
komorebic.exe toggle-focus-follows-mouse --implementation komorebi
```

#### Mouse Follows Focus

By default, the mouse will move to the center of the window when the focus is changed in a given direction. This
behaviour is know is 'mouse follows focus'. To disable this behaviour across all workspaces, add the following command
to your configuration file:

```powershell
komorebic.exe mouse-follows-focus disable
```

[![Watch the tutorial video](https://img.youtube.com/vi/LBoyXQiNINc/hqdefault.jpg)](https://www.youtube.com/watch?v=LBoyXQiNINc)

#### Saving and Loading Resized Layouts

If you create a BSP layout through various resize adjustments that you want to be able to restore easily in the future,
it is possible to "quicksave" that layout to the system's temporary folder and load it later in the same session, or
alternatively, you may save it to a specific file to be loaded again at any point in the future.

```powershell
komorebic.exe quick-save # saves the focused workspace to $Env:TEMP\komorebi.quicksave.json
komorebic.exe quick-load # loads $Env:TEMP\komorebi.quicksave.json on the focused workspace

komorebic.exe save ~/layouts/primary.json # saves the focused workspace to $Env:USERPROFILE\layouts\primary.json
komorebic.exe load ~/layouts/secondary.json # loads $Env:USERPROFILE\layouts\secondary.json on the focused workspace
```

These layouts can be applied to arbitrary collections of windows on any workspace, as they only track the layout
dimensions and are not coupled to the applications that were running at the time of saving.

When layouts that expect more or less windows than the number currently on the focused workspace are loaded, `komorebi`
will automatically reconcile the difference.

#### Creating and Loading Custom Layouts

Particularly for users of ultrawide monitors, traditional tiling layouts may not seem like the most efficient use of
screen space. If you feel this is the case with any of the default layouts, you are also welcome to create your own
custom layouts and save them as JSON or YAML.

If you're not comfortable writing the layouts directly in JSON or YAML, you can use
the [komorebi Custom Layout Generator](https://lgug2z.github.io/komorebi-custom-layout-generator/) to interactively
define a custom layout, and then copy the generated JSON content.

Custom layouts can be loaded on the current workspace or configured for a specific workspace with the following
commands:

```powershell
komorebic.exe load-custom-layout ~/custom.yaml
komorebic.exe workspace-custom-layout 0 0 ~/custom.yaml
```

The fundamental building block of a custom _komorebi_ layout is the Column.

Columns come in three variants:

- **Primary**: This is where your primary focus will be on the screen most of the time. There must be exactly one Primary
  Column in any custom layout. Optionally, you can specify the percentage of the screen width that you want the Primary
  Column to occupy.
- **Secondary**: This is an optional column that can either be full height of split horizontally into a fixed number of
  maximum rows. There can be any number of Secondary Columns in a custom layout.
- **Tertiary**: This is the final column where any remaining windows will be split horizontally into rows as they get added.

If there is only one window on the screen when a custom layout is selected, that window will take up the full work area
of the screen.

If the number of windows is equal to or less than the total number of columns defined in a custom layout, the windows
will be arranged in an equal-width columns.

When the number of windows is greater than the number of columns defined in the custom layout, the windows will begin to
be arranged according to the constraints set on the Primary and Secondary columns of the layout.

Here is an example custom layout that can be used as a starting point for your own:

YAML

```yaml
- column: Secondary
  configuration: !Horizontal 2 # max number of rows
- column: Primary
  configuration: !WidthPercentage 50 # percentage of screen
- column: Tertiary
  configuration: Horizontal
```

[![Watch the tutorial video](https://img.youtube.com/vi/SgmBHKEOcQ4/hqdefault.jpg)](https://www.youtube.com/watch?v=SgmBHKEOcQ4)

#### Dynamically Changing Layouts Based on Number of Visible Window Containers

With `komorebi` it is possible to define rules to automatically change the layout on a specified workspace when a
threshold of window containers is met.

```powershell
# On the first workspace of the first monitor (0 0)
# When there are one or more window containers visible on the screen (1)
# Use the bsp layout (bsp)
komorebic workspace-layout-rule 0 0 1 bsp

# On the first workspace of the first monitor (0 0)
# When there are five or more window containers visible on the screen (five)
# Use the custom layout stored in the home directory (~/custom.yaml)
komorebic workspace-custom-layout-rule 0 0 5 ~/custom.yaml
```

However, if you add workspace layout rules, you will not be able to manually change the layout of a workspace until all
layout rules for that workspace have been cleared.

```powershell
# If you decide that workspace layout rules are not for you, you can remove them from that same workspace like this
komorebic clear-workspace-layout-rules 0 0
```

## Development

If you would like to contribute code to this repository, there are a few requests that I have to ensure a foundation of
code quality, consistency and commit hygiene:

- Flatten all `use` statements
- Run `cargo +nightly clippy` and ensure that all lints and suggestions have been addressed before committing
- Run `cargo +nightly fmt --all` to ensure consistent formatting before committing
- Use `git cz` with
  the [Commitizen CLI](https://github.com/commitizen/cz-cli#conventional-commit-messages-as-a-global-utility) to prepare
  commit messages
- Provide at least one short sentence or paragraph in your commit message body to describe your thought process for the
  changes being committed

If you use IntelliJ, you should enable the following settings to ensure that code generated by macros is recognised by
the IDE for completions and navigation:

- Set `Expand declarative macros`
  to `Use new engine` under "Settings > Langauges & Frameworks > Rust"
- Enable the following experimental features:
  - `org.rust.cargo.evaluate.build.scripts`
  - `org.rust.macros.proc`

## Logs and Debugging

Logs from `komorebi` will be appended to `%LOCALAPPDATA%/komorebi/komorebi.log`; this file is never rotated or overwritten, so it will keep
growing until it is deleted by the user.

Whenever running the `komorebic stop` command or sending a Ctrl-C signal to `komorebi` directly, the `komorebi` process
ensures that all hidden windows are restored before termination.

If however, you ever end up with windows that are hidden and cannot be restored, a list of window handles known
to `komorebi` are stored and continuously updated in `%LOCALAPPDATA%/komorebi//komorebi.hwnd.json`.

### Restoring Windows

Running `komorebic restore-windows` will read the list of window handles and forcibly restore them, regardless of
whether the main `komorebi` process is running.

### Panics and Deadlocks

If `komorebi` ever stops responding, it is most likely either due to either a panic or a deadlock. In the case of a
panic, this will be reported in the log. In the case of a deadlock, there will not be any errors in the log, but the
process and the log will appear frozen.

If you believe you have encountered a deadlock, you can compile `komorebi` with `--features deadlock_detection` and try
reproducing the deadlock again. This will check for deadlocks every 5 seconds in the background, and if a deadlock is
found, information about it will appear in the log which can be shared when opening an issue.

## Window Manager State and Integrations

The current state of the window manager can be queried using the `komorebic state` command, which returns a JSON
representation of the `State` struct, which includes the current state of `WindowManager`.

This may also be polled to build further integrations and widgets on top of (if you ever wanted to build something
like [Stackline](https://github.com/AdamWagner/stackline) for Windows, you could do it by polling this command).

## Window Manager Event Subscriptions

It is also possible to subscribe to notifications of every `WindowManagerEvent` and `SocketMessage` handled
by `komorebi` using [Named Pipes](https://docs.microsoft.com/en-us/windows/win32/ipc/named-pipes).

First, your application must create a named pipe. Once the named pipe has been created, run the following command:

```powershell
komorebic.exe subscribe <your pipe name>
```

Note that you do not have to include the full path of the named pipe, just the name.

If the named pipe exists, `komorebi` will start pushing JSON data of successfully handled events and messages:

```json lines
{"event":{"type":"AddSubscriber","content":"yasb"},"state":{}}
{"event":{"type":"FocusWindow","content":"Left"},"state":{}}
{"event":{"type":"FocusChange","content":["SystemForeground",{"hwnd":131444,"title":"komorebi – README.md","exe":"idea64.exe","class":"SunAwtFrame","rect":{"left":13,"top":60,"right":1520,"bottom":1655}}]},"state":{}}
{"event":{"type":"MonitorPoll","content":["ObjectCreate",{"hwnd":5572450,"title":"OLEChannelWnd","exe":"explorer.exe","class":"OleMainThreadWndClass","rect":{"left":0,"top":0,"right":0,"bottom":0}}]},"state":{}}
{"event":{"type":"FocusWindow","content":"Right"},"state":{}}
{"event":{"type":"FocusChange","content":["SystemForeground",{"hwnd":132968,"title":"Windows PowerShell","exe":"WindowsTerminal.exe","class":"CASCADIA_HOSTING_WINDOW_CLASS","rect":{"left":1539,"top":60,"right":1520,"bottom":821}}]},"state":{}}
{"event":{"type":"FocusWindow","content":"Down"},"state":{}}
{"event":{"type":"FocusChange","content":["SystemForeground",{"hwnd":329264,"title":"den — Mozilla Firefox","exe":"firefox.exe","class":"MozillaWindowClass","rect":{"left":1539,"top":894,"right":1520,"bottom":821}}]},"state":{}}
{"event":{"type":"FocusWindow","content":"Up"},"state":{}}
{"event":{"type":"FocusChange","content":["SystemForeground",{"hwnd":132968,"title":"Windows PowerShell","exe":"WindowsTerminal.exe","class":"CASCADIA_HOSTING_WINDOW_CLASS","rect":{"left":1539,"top":60,"right":1520,"bottom":821}}]},"state":{}}
```

You may then filter on the `type` key to listen to the events that you are interested in. For a full list of possible
notification types, refer to the enum variants of `WindowManagerEvent` in `komorebi` and `SocketMessage`
in `komorebi-core`.

An example of how to create a named pipe and a subscription to `komorebi`'s handled events in Python
by [@denBot](https://github.com/denBot) can be
found [here](https://gist.github.com/denBot/4136279812f87819f86d99eba77c1ee0).

An example of how to create a named pipe and a subscription to `komorebi`'s handled events in Rust can also be found
in the [`komokana`](https://github.com/LGUG2Z/komokana) repository.

### Subscription Event Notification Schema

A [JSON Schema](https://json-schema.org/) of the event notifications emitted to subscribers can be generated with
the `komorebic notification-schema` command. The output of this command can be redirected to the clipboard or a file,
which can be used with services such as [Quicktype](https://app.quicktype.io/) to generate type definitions in different
programming languages.

### Communication over TCP

A TCP listener can optionally be exposed on a port of your choosing with the `--tcp-port=N` flag. If this flag is not
provided to `komorebi` or `komorebic start`, no TCP listener will be created.

Once created, your client may send
any [SocketMessage](https://github.com/LGUG2Z/komorebi/blob/master/komorebi-core/src/lib.rs#L37) to `komorebi` in the
same way that `komorebic` would.

This can be used if you would like to create your own alternative to `komorebic` which incorporates scripting and
various middleware layers, and similarly it can be used if you would like to integrate `komorebi` with
a [custom input handler](https://github.com/LGUG2Z/komorebi/issues/176#issue-1302643961).

If a client sends an unrecognized message, it will be disconnected and have to reconnect before trying to communicate
again.

### Socket Message Schema

A [JSON Schema](https://json-schema.org/) of socket messages used to send instructions to `komorebi` can be generated
with the `komorebic socket-schema` command. The output of this command can be redirected to the clipboard or a file,
which can be used with services such as [Quicktype](https://app.quicktype.io/) to generate type definitions in different
programming languages.

## Appreciations

- First and foremost, thank you to my wife, both for naming this project and for her patience throughout its never-ending development

- Thank you to [@sitiom](https://github.com/sitiom) for being [an exemplary open source community leader](https://jeezy.substack.com/p/the-open-source-contributions-i-appreciate)

- Thank you to the developers of [nog](https://github.com/TimUntersberger/nog) who came before me and whose work taught me more than I can ever hope to repay

- Thank you to the developers of [GlazeWM](https://github.com/lars-berger/GlazeWM) for pushing the boundaries of tiling window management on Windows with me and having an excellent spirit of collaboration

- Thank you to [@Ciantic](https://github.com/Ciantic) for helping me bring the [hidden Virtual Desktops cloaking function](https://github.com/Ciantic/AltTabAccessor/issues/1) to `komorebi`
