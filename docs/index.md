# komorebi for KDE

## Overview

`komorebi` is a tiling window manager for KDE Plasma on Wayland. It decides where each window
goes, and KWin, the window manager of Plasma, moves and draws the windows.

`komorebi` is a daemon that runs in the background, and it is controlled with a CLI,
`komorebic`. Every `komorebic` command that asks something of the daemon is a message on its
socket, so anything that can run a command can drive `komorebi`: a terminal, a script, or a
program that binds keys, which is how a key is given an action.

`komorebi` aims to make _as few modifications as possible_ to the operating system and desktop
environment by default. Users are free to make such modifications in their own configuration
files for `komorebi`, but these will always remain opt-in and off-by-default.

## How it fits together

_komorebi_ only responds to the events KWin reports about windows and to the messages it
receives on a dedicated socket. _komorebic_ is a CLI that writes messages on _komorebi_'s
socket. _komorebi_ doesn't handle any keyboard or mouse inputs: a program that binds keys,
such as the custom shortcuts in Plasma's System Settings, runs _komorebic_ commands.

```text
               PROCESS                SOCKET
key bindings  -------->  komorebic  <------>  komorebi
```

## Getting started

[Installing komorebi](installation.md) explains how to install the two programs: on NixOS with
the flake's module, which also starts komorebi with every Plasma session, and from source
anywhere else. [Running komorebi](usage/running-komorebi.md) explains how to start and stop the
daemon, ask it what it is doing and read its log.
[Focusing windows](usage/focusing-windows.md) explains how to give the focus to another window,
[Moving windows](usage/moving-windows.md) how to move a window to another tile,
[Stacking windows](usage/stacking-windows.md) how to put windows on one tile, have the others wait
out of sight and bring one of them to the front,
[Layouts](usage/layouts.md) how to change the arrangement of the tiles,
[Resizing windows](usage/resizing-windows.md) how to change the size of one (and
[Resizing a tile by dragging its edge](drag-resize.md) what a drag with the pointer does),
[Floating windows](usage/floating-windows.md) how to take a window out of the layout and work with
the floating ones,
[Monocle](usage/monocle.md) how to give one window the whole work area,
[Maximizing windows](usage/maximizing-windows.md) how to have KWin maximize one, and what
komorebi does when you maximize one yourself,
[Fullscreen windows](usage/fullscreen-windows.md) what komorebi does with one,
[Managing windows](usage/managing-windows.md) how to minimize, close and unmanage the focused
window and pause the daemon, and
[Padding and work area offsets](usage/padding-and-offsets.md) how to put gaps around the tiles
and keep parts of a monitor free of them.
[Rules](usage/rules.md) explains how to keep an application out of the layout, always tile it, or
float its windows.
[Initial window placement rules](initial-window-placement-rules.md) explains how to say where in the
layout a window goes when it opens.
[Workspaces](usage/workspaces.md) explains how each monitor shows one of its workspaces, which
are KWin's virtual desktops, and how to show another or move a window to one, and
[Workspace rules](usage/workspace-rules.md) how to have the windows of an application go to a
workspace by themselves.
[Configuration](usage/configuration.md) explains `komorebi.json`, where komorebi looks for it and
how it is reloaded when it changes, [Example configurations](example-configurations.md) has one to
start from, and [Application specific configuration](usage/application-specific-configuration.md)
explains the files of rules for particular applications it can name.
[Quickstart and autostart](usage/quickstart-and-autostart.md) explains how to write an example
configuration, how to start komorebi with Plasma without the module, and how to replace the
configuration of a running daemon.
[Borders](usage/borders.md) explains how komorebi has KWin draw a border around each window,
[Animation](usage/animation.md) how it has KWin animate the windows going to their places, and
[Transparency](usage/transparency.md) how it makes the windows that do not have the focus
see-through, [Focus follows mouse](usage/focus-follows-mouse.md) how it has KWin give the focus
to the window under the pointer, [Themes](usage/themes.md) how a theme gives the borders their
colors, and [Wallpapers](usage/wallpapers.md) how a monitor or a workspace shows a picture of its own,
and what it does to the borders.
[Changing monitors](usage/changing-monitors.md) explains what komorebi does when a monitor
changes size, is unplugged or is plugged in, and
[Sleeping and locking](usage/sleeping-and-locking.md) what it does when the machine wakes up and
the screen is unlocked.
[Subscribing to notifications](usage/subscribing-to-notifications.md) explains how a program
such as a status bar follows what komorebi does. The [CLI reference](cli/start.md) has a page
for every `komorebic` command, generated from its help.

## Community

There is a [Discord server](https://discord.gg/mGkn66PHkx) available for
`komorebi`-related discussion, help, troubleshooting etc.

There is a [YouTube
channel](https://www.youtube.com/channel/UCeai3-do-9O4MNy9_xjO6mg) where I post
`komorebi` development videos, feature previews and release overviews. Subscribing
to the channel (which is monetized as part of the YouTube Partner Program) and
watching videos is a really simple and passive way to contribute financially to
the development and maintenance of `komorebi`.

There is an [Awesome List](https://github.com/LGUG2Z/awesome-komorebi) which
showcases the many awesome projects that exist in the `komorebi` ecosystem.

## Licensing for Personal Use

`komorebi` is licensed under the [Komorebi 2.0.0 license](https://github.com/LGUG2Z/komorebi-license), which is a fork
of the [PolyForm Strict 1.0.0 license](https://polyformproject.org/licenses/strict/1.0.0). On a high level this means
that you are free to do whatever you want with `komorebi` for personal use other than redistribution, or distribution of
new works (i.e. hard-forks) based on the software.

Anyone is free to make their own fork of `komorebi` with changes intended either for personal use or for integration
back upstream via pull requests.

The [Komorebi 2.0.0 License](https://github.com/LGUG2Z/komorebi-license) does not permit any kind of commercial use (
i.e. using `komorebi` at work).

## Sponsorship for Personal Use

`komorebi` is a free and educational source project, and one that encourages you
to make charitable donations if you find the software to be useful and have the
financial means.

I encourage you to make a charitable donation to the [Palestine Children's
Relief Fund](https://pcrf1.app.neoncrm.com/forms/gaza-recovery) or to contribute
to a [Gaza Funds campaign](https://gazafunds.com) before you consider sponsoring
me on GitHub.

[GitHub Sponsors is enabled for this
project](https://github.com/sponsors/LGUG2Z). Sponsors can claim custom roles on
the Discord server, get shout-outs at the end of _komorebi_-related videos on
YouTube, and gain the ability to submit feature requests on the issue tracker.

If you would like to tip or sponsor the project but are unable to use GitHub
Sponsors, you may also sponsor through [Ko-fi](https://ko-fi.com/lgug2z), or
make an anonymous Bitcoin donation to `bc1qv73wzspc77k46uty4vp85x8sdp24mphvm58f6q`.

## Licensing for Commercial Use

A dedicated Individual Commercial Use License is available for those who want to
use `komorebi` at work.

The Individual Commercial Use License adds “Commercial Use” as a “Permitted Use”
for the licensed individual only, for the duration of a valid paid license
subscription only. All provisions and restrictions enumerated in the [Komorebi
License](https://github.com/LGUG2Z/komorebi-license) continue to apply.

More information, pricing and purchase links for Individual Commercial Use
Licenses [can be found here](https://lgug2z.com/software/komorebi).
