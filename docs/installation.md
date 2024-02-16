# Getting started

`komorebi` is a tiling window manager for Windows that is comprised comprised
of two main binaries, `komorebi.exe`, which contains the window manager itself,
and `komorebic.exe`, which is the main way to send commands to the tiling
window manager.

It is important to note that neither `komorebi.exe` or `komorebic.exe` handle
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

## Installation

`komorebi` is available pre-built to install via
[Scoop](https://scoop.sh/#/apps?q=komorebi) and
[WinGet](https://winget.run/pkg/LGUG2Z/komorebi), and you may also built
it from [source](https://github.com/LGUG2Z/komorebi) if you would prefer.

 - [Scoop](#scoop)
 - [WinGet](#winget)
 - [Building from source](#building-from-source)

## Long path support

It highly recommended that you enable support for long paths in Windows by
running the following command in an Administrator Terminal before installing
`komorebi`.

```powershell
Set-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name 'LongPathsEnabled' -Value 1
```

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
```

If the binaries have been built and added to your `$PATH` correctly, you should
see some output when running `komorebi --help` and `komorebic --help`
