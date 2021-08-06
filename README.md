# komorebi

Tiling Window Management for Windows.

![demo](https://s2.gifyu.com/images/ezgif-1-a21b17f39d06.gif)

## About

*komorebi* is a tiling window manager that works as an extension to
Microsoft's [Desktop Window Manager](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview) in Windows 10 and
above.

*komorebi* allows you to control application windows, virtual workspaces and display monitors with a CLI which can be used
with third-party software such as [AutoHotKey](https://github.com/Lexikos/AutoHotkey_L) to set user-defined keyboard
shortcuts.

## Description

*komorebi* only responds to [WinEvents](https://docs.microsoft.com/en-us/windows/win32/winauto/event-constants) and the
messages it receives on a dedicated socket.

*komorebic* is a CLI that writes messages on *komorebi*'s socket.

*komorebi* doesn't handle any keyboard or mouse inputs; a third party program (e.g. AutoHotKey) is needed in order to
translate keyboard and mouse events to *komorebic* commands.

This architecture, popularised by [*bspwm*](https://github.com/baskerville/bspwm) on Linux and
[*yabai*](https://github.com/koekeishiya/yabai) on macOS, is outlined as follows:

```
     PROCESS                SOCKET
ahk  -------->  komorebic  <------>  komorebi
```

## Design

*komorebi* is the successor to [*yatta*](https://github.com/LGUG2Z/yatta) and as such aims to build on the learnings
from that project.

While *yatta* was primary an attempt to learn how to work with and call Windows APIs from Rust, while secondarily
implementing a minimal viable tiling window manager for my own needs (largely single monitor, single workspace),
*komorebi* has been redesigned from the ground-up to support more complex features that have become standard in tiling
window managers on other platforms.

*komorebi* holds a list of physical monitors.

A monitor is just a rectangle of the available work area which contains one or more virtual workspaces.

A workspace holds a list of containers.

A container is just a rectangle where one or more application windows can be displayed.

This means that:

* Every monitor has its own collection of virtual workspaces
* Workspaces only know about containers and their dimensions, not about individual application windows
* Every application window must belong to a container, even if that container only contains one application window
* Many application windows can be stacked and cycled through in the same container within a workspace

## Getting Started

This project is still heavily under development and there are no prebuilt binaries available yet.

If you would like to use *komorebi*, you will need
a [working Rust development environment on Windows 10](https://rustup.rs/). If you are using
the `x86_64-pc-windows-msvc` toolchain, make sure you have also installed
the [Build Tools for Visual Studio 2019](https://stackoverflow.com/a/55603112).

You can then clone this repo and compile the source code to install the binaries for `komorebi` and `komorebic`:

```powershell
cargo install --path komorebi
cargo install --path komorebic
```

By running `komorebic start` at a Powershell prompt, you should see the following output:

```
Start-Process komorebi -WindowStyle hidden
```

This means that `komorebi` is now running in the background, tiling all your windows, and listening for commands sent to it
by `komorebic`.

You can similarly stop the process by running `komorebic stop`.

## Configuration

As previously mentioned, this project does not handle anything related to keybindings and shortcuts directly. I
personally use AutoHotKey to manage my window management shortcuts, and have provided a
sample [komorebi.ahk](komorebi.sample.ahk) AHK script that you can use as a starting point for your own.

## Development

If you would like to contribute code to this repository, there are a few requests that I have to ensure a foundation of
code quality, consistency and commit hygiene:

* Flatten all `use` statements except in `bindings/build.rs`
* Run `cargo +nightly clippy` and ensure that all lints and suggestions have been addressed before committing
* Run `cargo +nightly fmt --all` to ensure consistent formatting before committing
* Use `git cz` with
  the [Commitizen CLI](https://github.com/commitizen/cz-cli#conventional-commit-messages-as-a-global-utility) to prepare
  commit messages
* Provide at least one short sentence or paragraph in your commit message body to describe your thought process for the
  changes being committed