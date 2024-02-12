# komorebi

Tiling Window Management for Windows.

<p>
  <a href="https://techforpalestine.org/learn-more">
    <img alt="Tech for Palestine" src="https://badge.techforpalestine.org/default">
  </a>
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
  <a href="https://ko-fi.com/lgug2z">
    <img alt="Ko-fi" src="https://img.shields.io/badge/kofi-tip-green">
  </a>
  <a href="https://notado.app/feeds/jado/software-development">
    <img alt="Notado Feed" src="https://img.shields.io/badge/Notado-Subscribe-informational">
  </a>
  <a href="https://www.youtube.com/channel/UCeai3-do-9O4MNy9_xjO6mg?sub_confirmation=1">
    <img alt="YouTube" src="https://img.shields.io/youtube/channel/subscribers/UCeai3-do-9O4MNy9_xjO6mg">
  </a>
</p>

![screenshot](https://user-images.githubusercontent.com/13164844/184027064-f5a6cec2-2865-4d65-a549-a1f1da589abf.png)

- [About](#about)
- [Charitable Donations](#charitable-donations)
- [GitHub Sponsors](#github-sponsors)
- [Demonstrations](#demonstrations)
- [Getting Started](#getting-started)
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

_komorebi_ is a tiling window manager that works as an extension to Microsoft's
[Desktop Window
Manager](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview) in
Windows 10 and above.

_komorebi_ allows you to control application windows, virtual workspaces and
display monitors with a CLI which can be used with third-party software such as
[AutoHotKey](https://github.com/Lexikos/AutoHotkey_L) to set user-defined
keyboard shortcuts.

_komorebi_ aims to make _as few modifications as possible_ to the operating
system and desktop environment by default. Users are free to make such
modifications in their own configuration files for _komorebi_, but these will
remain opt-in and off-by-default for the foreseeable future.

There is a [Discord server](https://discord.gg/mGkn66PHkx) available for
_komorebi_-related discussion, help, troubleshooting etc. If you have any
specific feature requests or bugs to report, please create an issue in this
repository.

There is a [YouTube
channel](https://www.youtube.com/channel/UCeai3-do-9O4MNy9_xjO6mg) where I post
_komorebi_ development videos. If you would like to be notified of upcoming
videos please subscribe and turn on notifications.

Articles, blog posts, demos, and videos about _komorebi_ can be added to this
list by PR:

- [Moving to Windows from Linux Pt 1](https://kvwu.io/posts/moving-to-windows/)
- [Windows 下的现代化平铺窗口管理器 komorebi](https://zhuanlan.zhihu.com/p/455064481)
- [komorebi を導入してみる](https://zenn.dev/omochice/articles/50f42a3df8f426)

## Getting Started

A [detailed quickstart
guide](https://lgug2z.github.io/komorebi/installation.html) is available on the
documentation website.

## Charitable Donations

_komorebi_ is a free and open-source project, and one that encourages you to
make charitable donations if you find the software to be useful and have the
financial means.

I encourage you to make a charitable donation to the [Palestine Children's
Relief Fund](https://pcrf1.app.neoncrm.com/forms/gaza-recovery) before you
consider sponsoring me on GitHub.

## Project Sponsorship

[GitHub Sponsors is enabled for this
project](https://github.com/sponsors/LGUG2Z). Unfortunately I don't have
anything specific to offer besides my gratitude and shout outs at the end of
_komorebi_ live development videos and tutorials.

If you would like to tip or sponsor the project but are unable to use GitHub
Sponsors, you may also sponsor through [Ko-fi](https://ko-fi.com/lgug2z).

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
