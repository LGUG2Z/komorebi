# Getting started

`komorebi` is a tiling window manager for Windows that is comprised of two
main binaries, `komorebi.exe`, which contains the window manager itself,
and `komorebic.exe`, which is the main way to send commands to the tiling
window manager.

It is important to note that neither `komorebi.exe` nor `komorebic.exe` handle
key bindings, because `komorebi` is a tiling window manager and not a hotkey
daemon.

This getting started guide suggests the installation of
[`whkd`](https://github.com/LGUG2Z/whkd) to allow you to bind `komorebic.exe`
commands to hotkeys to allow you to communicate with the tiling window manager
using keyboard shortcuts.

However, `whkd` is a very simple hotkey daemon, and notably, does not include
workarounds for Microsoft's restrictions on hotkey combinations that can use
the `Windows` key.

If using hotkey combinations with the `Windows` key is important to you, I
suggest that once you are familiar with the main `komorebic.exe` commands used
to manipulate the window manager, you use
[AutoHotKey](https://www.autohotkey.com/) to handle your key bindings.

`komorebi` also includes `komorebi-bar.exe`, a simple and reliable status bar which
is deeply integrated with the tiling window manager, and can be customized with
various widgets and themes.

## Installation

`komorebi` is available pre-built to install via
[Scoop](https://scoop.sh/#/apps?q=komorebi) and
[WinGet](https://winget.run/pkg/LGUG2Z/komorebi), and you may also build
it from [source](https://github.com/LGUG2Z/komorebi) if you would prefer.

- [Scoop](#scoop)
- [WinGet](#winget)
- [Building from source](#building-from-source)
- [Offline](#offline)

## Long path support

It is highly recommended that you enable support for long paths in Windows by
running the following command in an Administrator Terminal before installing
`komorebi`.

```powershell
Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -Value 1
```

## Disabling unnecessary system animations

It is highly recommended that you enable the "Turn off all unnecessary animations (when possible)" option in
"Control Panel > Ease of Access > Ease of Access Centre / Make the computer easier to see" for the best performance with
komorebi.

## Scoop

Make sure you have installed [`scoop`](https://scoop.sh) and verified that
installed binaries are available in your `$PATH` before proceeding.

Issues with `komorebi` and related commands not being recognized in the
terminal ultimately come down to the `$PATH` environment variable not being
correctly configured by your package manager and **should not** be raised as
bugs or issues either on the `komorebi` GitHub repository or Discord server.

### Install komorebi and whkd

First add the extras bucket

```powershell
scoop bucket add extras
```

Then install the `komorebi` and `whkd` packages using `scoop install`

```powershell
scoop install komorebi whkd
```

Once komorebi is installed, proceed to get the [example
configurations](example-configurations.md).

## WinGet

Make sure you have installed the latest version of
[`winget`](https://learn.microsoft.com/en-us/windows/package-manager/winget/)
and verified that installed binaries are available in your `$PATH` before
proceeding.

Issues with `komorebi` and related commands not being recognized in the
terminal ultimately come down to the `$PATH` environment variable not being
correctly configured by your package manager and **should not** be raised as
bugs or issues either on the `komorebi` GitHub repository or Discord server.

### Install komorebi and whkd

Install the `komorebi` and `whkd` packages using `winget install`

```powershell
winget install LGUG2Z.komorebi
winget install LGUG2Z.whkd
```

Once komorebi is installed, proceed to get the [example
configurations](example-configurations.md).

## Building from source

Make sure you have installed [`rustup`](https://rustup.rs), a stable `rust`
compiler toolchain, and the Visual Studio [Visual Studio
prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html).

Clone the git repository, enter the directory, and build the following binaries:

```powershell
cargo +stable install --path komorebi --locked
cargo +stable install --path komorebic --locked
cargo +stable install --path komorebic-no-console --locked
cargo +stable install --path komorebi-gui --locked
cargo +stable install --path komorebi-bar --locked
cargo +stable install --path komorebi-shortcuts --locked
```

If the binaries have been built and added to your `$PATH` correctly, you should
see some output when running `komorebi --help` and `komorebic --help`

### Offline

Download the latest [komorebi](https://github.com/LGUG2Z/komorebi/releases)
and [whkd](https://github.com/LGUG2Z/whkd/releases) MSI installers on an internet-connected computer, then copy them to
an offline machine to install.

Once installed, proceed to get the [example configurations](example-configurations.md) (none of the commands for
first-time set up and running komorebi require an internet connection).

## Upgrades

Before upgrading, make sure to run `komorebic stop --whkd --bar`. This is to ensure that all the current
komorebi-related exe files can be replaced without issue.

Then, depending on whether you installed via `scoop` or `winget`, you can run the appropriate command:

```powershell
# for winget
winget upgrade LGUG2Z.komorebi
```

```powershell
# for scoop
scoop update komorebi
```

Once the upgrade is completed you can confirm that you have the latest version by running `komorebic --version`, and
then start it with `komorebic start --whkd --bar`.

## Uninstallation

Before uninstalling, first run `komorebic stop --whkd --bar` to make sure that
the `komorebi`, `komorebi-bar` and `whkd` processes have been stopped.

Then, depending on whether you installed with Scoop or WinGet, run `scoop
uninstall komorebi whkd` or `winget uninstall LGUG2Z.komorebi LGUG2Z.whkd`.

Finally, you can run the following commands in a PowerShell prompt to clean up
files created by the `quickstart` command and any other runtime files:

```powershell
rm $Env:USERPROFILE\komorebi.json
rm $Env:USERPROFILE\applications.json
rm $Env:USERPROFILE\.config\whkdrc
rm -r -Force $Env:LOCALAPPDATA\komorebi
```
