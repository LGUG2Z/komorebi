# komorebi

Tiling Window Management for Windows.

![screenshot](https://i.ibb.co/BTqNS45/komorebi.png)

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

## Features

- [x] Multi-monitor
- [x] Virtual workspaces
- [x] Window stacks
- [x] Cycle through stacked windows
- [x] Change focused window by direction
- [x] Move focused window container in direction
- [x] Move focused window container to monitor
- [x] Move focused window container to workspace
- [x] Mouse drag to swap window container position
- [x] Configurable workspace and container gaps
- [x] BSP tree layout
- [x] Flip BSP tree layout horizontally or vertically
- [x] Equal-width, max-height column layout
- [x] Floating rules based on exe name
- [x] Floating rules based on window title
- [x] Floating rules based on window class
- [x] Toggle floating windows
- [x] Toggle monocle window
- [x] Pause all window management
- [x] View window manager state
- [ ] Configure split ratio like *bspwm*

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
  
## Logs and Debugging

Logs from `komorebi` will be appended to `~/komorebi.log`; this file is never rotated or overwritten, so it will keep
growing until it is deleted by the user.

Whenever running the `komorebic stop` command or sending a Ctrl-C signal to `komorebi` directly, the `komorebi` process
ensures that all hidden windows are restored before termination.

If however, you ever end up with windows that are hidden and cannot be restored, a list of window handles known
to `komorebi` are stored and continuously updated in `~/komorebi.hwnd.json`.

Running `komorebic restore-windows` will read the list of window handles and forcibly restore them, regardless of
whether the main `komorebi` process is running.

## Window Manager State and Integrations

The current state of the window manager can be queried using the `komorebic state` command, which returns a JSON
representation of the `WindowManager` struct.

This may also be polled to build further integrations and widgets on top of (if you ever wanted to build something
like [Stackline](https://github.com/AdamWagner/stackline) for Windows, you could do it by polling this command).

```json
{
  "monitors": {
    "elements": [
      {
        "id": 65537,
        "monitor_size": {
          "left": 0,
          "top": 0,
          "right": 3840,
          "bottom": 2160
        },
        "work_area_size": {
          "left": 0,
          "top": 40,
          "right": 3840,
          "bottom": 2120
        },
        "workspaces": {
          "elements": [
            {
              "name": "bsp",
              "containers": {
                "elements": [
                  {
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 2623596,
                          "title": "komorebi – README.md",
                          "exe": "idea64.exe",
                          "class": "SunAwtFrame",
                          "rect": {
                            "left": 8,
                            "top": 60,
                            "right": 1914,
                            "bottom": 2092
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 198266,
                          "title": "LGUG2Z/komorebi: A(nother) tiling window manager for Windows 10 based on binary space partitioning - Mozilla Firefox",
                          "exe": "firefox.exe",
                          "class": "MozillaWindowClass",
                          "rect": {
                            "left": 1918,
                            "top": 60,
                            "right": 1914,
                            "bottom": 1042
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 1247352,
                          "title": "Windows PowerShell",
                          "exe": "WindowsTerminal.exe",
                          "class": "CASCADIA_HOSTING_WINDOW_CLASS",
                          "rect": {
                            "left": 1918,
                            "top": 1110,
                            "right": 959,
                            "bottom": 1042
                          }
                        }
                      ],
                      "focused": 0
                    }
                  },
                  {
                    "windows": {
                      "elements": [
                        {
                          "hwnd": 395464,
                          "title": "Signal",
                          "exe": "Signal.exe",
                          "class": "Chrome_WidgetWin_1",
                          "rect": {
                            "left": 2873,
                            "top": 1110,
                            "right": 959,
                            "bottom": 1042
                          }
                        }
                      ],
                      "focused": 0
                    }
                  }
                ],
                "focused": 2
              },
              "monocle_container": null,
              "floating_windows": [],
              "layout": "BSP",
              "layout_flip": null,
              "workspace_padding": 10,
              "container_padding": 10
            },
          ],
          "focused": 0
        }
      }
    ],
    "focused": 0
  },
  "is_paused": false
}
```