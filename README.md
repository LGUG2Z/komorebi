# komorebi

Tiling Window Management for Windows.

![screenshot](https://i.ibb.co/BTqNS45/komorebi.png)

## About

_komorebi_ is a tiling window manager that works as an extension to
Microsoft's [Desktop Window Manager](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview) in Windows 10 and
above.

_komorebi_ allows you to control application windows, virtual workspaces and display monitors with a CLI which can be
used with third-party software such as [AutoHotKey](https://github.com/Lexikos/AutoHotkey_L) to set user-defined
keyboard shortcuts.

Translations of this document can be found in the project wiki:

- [komorebi 中文用户指南](https://github.com/LGUG2Z/komorebi/wiki/README-zh) (by [@crosstyan](https://github.com/crosstyan))

## Description

_komorebi_ only responds to [WinEvents](https://docs.microsoft.com/en-us/windows/win32/winauto/event-constants) and the
messages it receives on a dedicated socket.

_komorebic_ is a CLI that writes messages on _komorebi_'s socket.

_komorebi_ doesn't handle any keyboard or mouse inputs; a third party program (e.g. AutoHotKey) is needed in order to
translate keyboard and mouse events to _komorebic_ commands.

This architecture, popularised by [_bspwm_](https://github.com/baskerville/bspwm) on Linux and
[_yabai_](https://github.com/koekeishiya/yabai) on macOS, is outlined as follows:

```
     PROCESS                SOCKET
ahk  -------->  komorebic  <------>  komorebi
```

## Design

_komorebi_ is the successor to [_yatta_](https://github.com/LGUG2Z/yatta) and as such aims to build on the learnings
from that project.

While _yatta_ was primary an attempt to learn how to work with and call Windows APIs from Rust, while secondarily
implementing a minimal viable tiling window manager for my own needs (largely single monitor, single workspace),
_komorebi_ has been redesigned from the ground-up to support more complex features that have become standard in tiling
window managers on other platforms.

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

### GitHub Releases

Prebuilt binaries are available on the [releases page](https://github.com/LGUG2Z/komorebi/releases) in a `zip` archive.
Once downloaded, you will need to move the `komorebi.exe` and `komorebic.exe` binaries to a directory in your `Path` (
you can see these directories by running `$Env:Path.split(";")` at a PowerShell prompt).

Alternatively, you may add a new directory to your `Path`
using [`setx`](https://docs.microsoft.com/en-us/windows-server/administration/windows-commands/setx) or the Environment
Variables pop up in System Properties Advanced (which can be launched with `SystemPropertiesAdvanced.exe` at a
PowerShell prompt), and then move the binaries to that directory.

### Scoop

If you use the [Scoop](https://scoop.sh/) command line installer, you can run the following commands to install the
binaries from the latest GitHub Release:

```
scoop bucket add komorebi https://github.com/LGUG2Z/komorebi-bucket
scoop install komorebi
```

If you install _komorebi_ using Scoop, the binaries will automatically be added to your `Path` and a command will be
shown for you to run in order to get started using the sample configuration file.

### Building from Source

If you prefer to compile _komorebi_ from source, you will need
a [working Rust development environment on Windows 10](https://rustup.rs/). The `x86_64-pc-windows-msvc` toolchain is
required, so make sure you have also installed
the [Build Tools for Visual Studio 2019](https://stackoverflow.com/a/55603112).

You can then clone this repo and compile the source code to install the binaries for `komorebi` and `komorebic`:

```powershell
cargo install --path komorebi --locked
cargo install --path komorebic --locked
```

### Running

Once you have either the prebuilt binaries in your `Path`, or have compiled the binaries from source (these will already
be in your `Path` if you installed Rust with [rustup](https://rustup.rs), which you absolutely should), you can
run `komorebic start` at a Powershell prompt, and you will see the following output:

```
Start-Process komorebi -WindowStyle hidden
```

This means that `komorebi` is now running in the background, tiling all your windows, and listening for commands sent to
it by `komorebic`. You can similarly stop the process by running `komorebic stop`.

### Configuring

Once `komorebi` is running, you can execute the `komorebi.sample.ahk` script to set up the default keybindings via AHK
(the file includes comments to help you start building your own configuration).

If you have AutoHotKey installed and a `komorebi.ahk` file in your home directory (run `$Env:UserProfile` at a
PowerShell prompt to find your home directory), `komorebi` will automatically try to load it when starting.

There is also tentative support for loading a AutoHotKey v2 files, if the file is named `komorebi.ahk2` and
the `AutoHotKey64.exe` executable for AutoHotKey v2 is in your `Path`. If both `komorebi.ahk` and `komorebi.ahk2` files
exist in your home directory, only `komorebi.ahk` will be loaded. An example of an AutoHotKey v2 configuration file
for _komorebi_ can be found [here](https://gist.github.com/crosstyan/dafacc0778dabf693ce9236c57b201cd).

### Common First-Time Tips

#### Floating Windows

Sometimes you will want a specific application to never be tiled, and instead float all the time. You add add rules to
enforce this behaviour:

```powershell
komorebic.exe float-rule title "Control Panel"
# komorebic.exe float-rule exe [EXE NAME]
# komorebic.exe float-rule class [CLASS NAME]
```

#### Windows Not Getting Managed

In some rare cases, a window may not automatically be registered to be managed by `komorebi`. When this happens, you can
manually add a rule to force `komorebi` to manage it:

```powershell
komorebic.exe manage-rule exe TIM.exe
# komorebic.exe manage-rule class [CLASS NAME]
# komorebic.exe manage-rule title [TITLE]
```

#### Tray Applications

If you are experiencing behaviour where
[closing a window leaves a blank tile, but minimizing the same window does not](https://github.com/LGUG2Z/komorebi/issues/6)
, you have probably enabled a 'close/minimize to tray' option for that application. You can tell _komorebi_ to handle
this application appropriately by identifying it via the executable name or the window class:

```powershell
komorebic.exe identify-tray-application exe Discord.exe
# komorebic.exe identify-tray-application class [CLASS NAME]
# komorebic.exe identify-tray-application title [TITLE]
```

#### Focus Follows Mouse

`komorebi` supports two focus-follows-mouse implementations; the native Windows Xmouse implementation, which treats the
desktop, the task bar, and the system tray as windows and switches focus to them eagerly, and a custom `komorebi`
implementation, which only considers windows managed by `komorebi` as valid targets to switch focus to when moving the
mouse.

When calling any of the `komorebic` commands related to focus-follows-mouse functionality, the `komorebi`
implementation will be chosen as the default implementation. You can optionally specify the `windows` implementation by
passing it as an argument to the `--implementation` flag:

```powershell
komorebic.exe toggle-focus-follows-mouse --implementation windows
```

## Configuration with `komorebic`

As previously mentioned, this project does not handle anything related to keybindings and shortcuts directly. I
personally use AutoHotKey to manage my window management shortcuts, and have provided a
sample [komorebi.ahk](komorebi.sample.ahk) AHK script that you can use as a starting point for your own.

You can run `komorebic.exe` to get a full list of the commands that you can use to customise `komorebi` and create
keybindings with. You can run `komorebic.exe <COMMAND> --help` to get a full explanation of the arguments required for
each command.

```
start                         Start komorebi.exe as a background process
stop                          Stop the komorebi.exe process and restore all hidden windows
state                         Show a JSON representation of the current window manager state
query                         Query the current window manager state
log                           Tail komorebi.exe's process logs (cancel with Ctrl-C)
focus                         Change focus to the window in the specified direction
move                          Move the focused window in the specified direction
stack                         Stack the focused window in the specified direction
resize                        Resize the focused window in the specified direction
unstack                       Unstack the focused window
cycle-stack                   Cycle the focused stack in the specified cycle direction
move-to-monitor               Move the focused window to the specified monitor
move-to-workspace             Move the focused window to the specified workspace
send-to-monitor               Send the focused window to the specified monitor
send-to-workspace             Send the focused window to the specified workspace
focus-monitor                 Focus the specified monitor
focus-workspace               Focus the specified workspace on the focused monitor
new-workspace                 Create and append a new workspace on the focused monitor
adjust-container-padding      Adjust container padding on the focused workspace
adjust-workspace-padding      Adjust workspace padding on the focused workspace
change-layout                 Set the layout on the focused workspace
flip-layout                   Flip the layout on the focused workspace (BSP only)
promote                       Promote the focused window to the top of the tree
retile                        Force the retiling of all managed windows
ensure-workspaces             Create at least this many workspaces for the specified monitor
container-padding             Set the container padding for the specified workspace
workspace-padding             Set the workspace padding for the specified workspace
workspace-layout              Set the layout for the specified workspace
workspace-tiling              Enable or disable window tiling for the specified workspace
workspace-name                Set the workspace name for the specified workspace
toggle-pause                  Toggle the window manager on and off across all monitors
toggle-tiling                 Toggle window tiling on the focused workspace
toggle-float                  Toggle floating mode for the focused window
toggle-monocle                Toggle monocle mode for the focused container
toggle-maximize               Toggle native maximization for the focused window
restore-windows               Restore all hidden windows (debugging command)
manage                        Force komorebi to manage the focused window
unmanage                      Unmanage a window that was forcibly managed
reload-configuration          Reload ~/komorebi.ahk (if it exists)
watch-configuration           Enable or disable watching of ~/komorebi.ahk (if it exists)
float-rule                    Add a rule to always float the specified application
manage-rule                   Add a rule to always manage the specified application
workspace-rule                Add a rule to associate an application with a workspace
identify-tray-application     Identify an application that closes to the system tray
identify-border-overflow      Identify an application that has overflowing borders
focus-follows-mouse           Enable or disable focus follows mouse for the operating system
toggle-focus-follows-mouse    Toggle focus follows mouse for the operating system
ahk-library                   Generate a library of AutoHotKey helper functions
help                          Print this message or the help of the given subcommand(s)
```

### AutoHotKey Helper Library for `komorebic`

Additionally, you may run `komorebic.exe ahk-library` to
generate [a helper library for AutoHotKey](komorebic.lib.sample.ahk) which wraps every `komorebic` command in a native
AHK function.

If you include the generated library at the top of your `~/komorebi.ahk` configuration file, you will be able to call
any of the functions that it contains. A sample AHK script that shows how this library can be
used [is available here](komorebi.sample.with.lib.ahk).

## Features

- [x] Multi-monitor
- [x] Virtual workspaces
- [x] Window stacks
- [x] Cycle through stacked windows
- [x] Change focused window by direction
- [x] Move focused window container in direction
- [x] Move focused window container to monitor and follow
- [x] Move focused window container to workspace follow
- [x] Send focused window container to monitor
- [x] Send focused window container to workspace
- [x] Mouse follows focused container
- [x] Resize window container in direction
- [ ] Resize child window containers by split ratio
- [x] Mouse drag to swap window container position
- [x] Mouse drag to resize window container
- [x] Configurable workspace and container gaps
- [x] BSP tree layout
- [x] Flip BSP tree layout horizontally or vertically
- [x] Equal-width, max-height column layout
- [x] Floating rules based on exe name, window title and class
- [x] Workspace rules based on exe name and window class
- [x] Additional manage rules based on exe name and window class
- [x] Identify applications which overflow their borders by exe name and class
- [x] Identify 'close/minimize to tray' applications by exe name and class
- [x] Toggle floating windows
- [x] Toggle monocle window
- [x] Toggle native maximization
- [x] Toggle Xmouse/Windows focus follows mouse implementation
- [x] Toggle Komorebi focus follows mouse implementation (desktop and system tray-aware)
- [x] Toggle automatic tiling
- [x] Pause all window management
- [x] Load configuration on startup
- [x] Manually reload configuration
- [x] Watch configuration for changes
- [x] Helper library for AutoHotKey
- [x] View window manager state
- [x] Query window manager state

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

Logs from `komorebi` will be appended to `~/komorebi.log`; this file is never rotated or overwritten, so it will keep
growing until it is deleted by the user.

Whenever running the `komorebic stop` command or sending a Ctrl-C signal to `komorebi` directly, the `komorebi` process
ensures that all hidden windows are restored before termination.

If however, you ever end up with windows that are hidden and cannot be restored, a list of window handles known
to `komorebi` are stored and continuously updated in `~/komorebi.hwnd.json`.

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
