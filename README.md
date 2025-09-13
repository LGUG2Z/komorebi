# komorebi

Tiling Window Management for Windows.

<p>
  <a href="https://techforpalestine.org/learn-more">
    <img alt="Tech for Palestine" src="https://badge.techforpalestine.org/default">
  </a>
  <img alt="GitHub Workflow Status" src="https://img.shields.io/github/actions/workflow/status/LGUG2Z/komorebi/.github/workflows/windows.yaml">
  <img alt="GitHub all releases" src="https://img.shields.io/github/downloads/LGUG2Z/komorebi/total">
  <img alt="GitHub commits since latest release (by date) for a branch" src="https://img.shields.io/github/commits-since/LGUG2Z/komorebi/latest">
  <img alt="Active Individual Commercial Use Licenses" src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Flgug2z-ecstaticmagentacheetah.web.val.run&query=%24.&label=active%20individual%20commercial%20use%20licenses&cacheSeconds=3600&link=https%3A%2F%2Flgug2z.com%2Fsoftware%2Fkomorebi">
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

## Overview

_komorebi_ is a tiling window manager that works as an extension to Microsoft's
[Desktop Window
Manager](https://docs.microsoft.com/en-us/windows/win32/dwm/dwm-overview) in
Windows 10 and above.

_komorebi_ allows you to control application windows, virtual workspaces and display monitors with a CLI which can be
used with third-party software such as [`whkd`](https://github.com/LGUG2Z/whkd)
and [AutoHotKey](https://github.com/Lexikos/AutoHotkey_L) to set user-defined keyboard shortcuts.

_komorebi_ aims to make _as few modifications as possible_ to the operating
system and desktop environment by default. Users are free to make such
modifications in their own configuration files for _komorebi_, but these will
remain opt-in and off-by-default for the foreseeable future.

Please refer to the [documentation](https://lgug2z.github.io/komorebi) for instructions on how
to [install](https://lgug2z.github.io/komorebi/installation.html) and
[configure](https://lgug2z.github.io/komorebi/example-configurations.html)
_komorebi_, [common workflows](https://lgug2z.github.io/komorebi/common-workflows/komorebi-config-home.html), a complete
[configuration schema reference](https://komorebi.lgug2z.com/schema) and a
complete [CLI reference](https://lgug2z.github.io/komorebi/cli/quickstart.html).

## Community

There is a [Discord server](https://discord.gg/mGkn66PHkx) available for
_komorebi_-related discussion, help, troubleshooting etc. If you have any
specific feature requests or bugs to report, please create an issue in this
repository.

There is a [YouTube
channel](https://www.youtube.com/channel/UCeai3-do-9O4MNy9_xjO6mg) where I post
_komorebi_ development videos, feature previews and release overviews. Subscribing
to the channel (which is monetized as part of the YouTube Partner Program) and
watching videos is a really simple and passive way to contribute financially to
the development and maintenance of _komorebi_.

There is an [Awesome List](https://github.com/LGUG2Z/awesome-komorebi) which
showcases the many awesome projects that exist in the _komorebi_ ecosystem.

## Licensing for Personal Use

`komorebi` is [educational source
software](https://lgug2z.com/articles/educational-source-software/).

`komorebi` is licensed under the [Komorebi 2.0.0
license](https://github.com/LGUG2Z/komorebi-license), which is a fork of the
[PolyForm Strict 1.0.0
license](https://polyformproject.org/licenses/strict/1.0.0). On a high level
this means that you are free to do whatever you want with `komorebi` for
personal use other than redistribution, or distribution of new works (i.e.
hard-forks) based on the software.

Anyone is free to make their own fork of `komorebi` with changes intended either
for personal use or for integration back upstream via pull requests.

The [Komorebi 2.0.0 License](https://github.com/LGUG2Z/komorebi-license) does
not permit any kind of commercial use (i.e. using `komorebi` at work).

## Sponsorship for Personal Use

_komorebi_ is a free and educational source project, and one that encourages you
to make charitable donations if you find the software to be useful and have the
financial means.

I encourage you to make a charitable donation to the [Palestine Children's
Relief Fund](https://pcrf1.app.neoncrm.com/forms/gaza-recovery) or to contribute
to a [Gaza Funds campaign](https://gazafunds.com) before you consider sponsoring
me on GitHub.

[GitHub Sponsors is enabled for this
project](https://github.com/sponsors/LGUG2Z). Sponsors can claim custom roles on
the Discord server, get shout outs at the end of _komorebi_-related videos on
YouTube, gain the ability to submit feature requests on the issue tracker, and
receive releases of komorebi with "easter eggs" on physical media.

If you would like to tip or sponsor the project but are unable to use GitHub
Sponsors, you may also sponsor through [Ko-fi](https://ko-fi.com/lgug2z), or
make an anonymous Bitcoin donation to `bc1qv73wzspc77k46uty4vp85x8sdp24mphvm58f6q`.

## Licensing for Commercial Use

A dedicated Individual Commercial Use License is available for those who want to
use `komorebi` at work.

The Individual Commerical Use License adds “Commercial Use” as a “Permitted Use”
for the licensed individual only, for the duration of a valid paid license
subscription only. All provisions and restrictions enumerated in the [Komorebi
License](https://github.com/LGUG2Z/komorebi-license) continue to apply.

More information, pricing and purchase links for Individual Commercial Use
Licenses [can be found here](https://lgug2z.com/software/komorebi).

# Installation

A [detailed installation and quickstart
guide](https://lgug2z.github.io/komorebi/installation.html) is available which shows how to get started
using `scoop`, `winget` or building from source.

[![Watch the quickstart walkthrough video](https://img.youtube.com/vi/MMZUAtHbTYY/hqdefault.jpg)](https://www.youtube.com/watch?v=MMZUAtHbTYY)

# Comparison With Fancy Zones

Community member [Olge](https://www.youtube.com/@polle5555) has created an
excellent video which compares the default window management features of
Windows 11, Fancy Zones and komorebi.

If you are not familiar with tiling window managers or if you are looking at
komorebi and wondering "how is this different from Fancy Zones? 🤔", this short
video will answer the majority of your questions.

[![Watch the comparison video](https://img.youtube.com/vi/0LCbS_gm0RA/hqdefault.jpg)](https://www.youtube.com/watch?v=0LCbS_gm0RA)

# Demonstrations

[@amnweb](https://github.com/amnweb) showing _komorebi_ `v0.1.28` running on Windows 11 with window borders,
unfocused window transparency and animations enabled, using a custom status bar integrated using
_komorebi_'
s [Window Manager Event Subscriptions](https://github.com/LGUG2Z/komorebi?tab=readme-ov-file#window-manager-event-subscriptions).

https://github.com/LGUG2Z/komorebi/assets/13164844/21be8dc4-fa76-4f70-9b37-1d316f4b40c2

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

# Contribution Guidelines

If you would like to contribute to `komorebi` please take the time to carefully
read the guidelines below.

Please see [CONTRIBUTING.md](./CONTRIBUTING.md) for more information about how
code contributions to `komorebi` are licensed.

## Commit hygiene

- Flatten all `use` statements
- Run `cargo +stable clippy` and ensure that all lints and suggestions have been addressed before committing
- Run `cargo +nightly fmt --all` to ensure consistent formatting before committing
- Use `git cz` with
  the [Commitizen CLI](https://github.com/commitizen/cz-cli#conventional-commit-messages-as-a-global-utility) to prepare
  commit messages
- Provide **at least** one short sentence or paragraph in your commit message body to describe your thought process for
  the changes being committed

## PRs should contain only a single feature or bug fix

It is very difficult to review pull requests which touch multiple unrelated features and parts of the codebase.

Please do not submit pull requests like this; you will be asked to separate them into smaller PRs that deal only with
one feature or bug fix at a time.

If you are working on multiple features and bug fixes, I suggest that you cut a branch called `local-trunk`
from `master` which you keep up to date, and rebase the various independent branches you are working on onto that branch
if you want to test them together or create a build with everything integrated.

## Refactors to the codebase must have prior approval

`komorebi` is a mature codebase with an internal consistency and structure that has developed organically over close to
half a decade.

There are [countless hours of live coding videos](https://youtube.com/@LGUG2Z) demonstrating work on this project and
showing new contributors how to do everything from basic tasks like implementing new `komorebic` commands to
distinguishing monitors by manufacturer hardware identifiers and video card ports.

Refactors to the structure of the codebase are not taken lightly and require prior discussion and approval.

Please do not start refactoring the codebase with the expectation of having your changes integrated until you receive an
explicit approval or a request to do so.

Similarly, when implementing features and bug fixes, please stick to the structure of the codebase as much as possible
and do not take this as an opportunity to do some "refactoring along the way".

It is extremely difficult to review PRs for features and bug fixes if they are lost in sweeping changes to the structure
of the codebase.

## Breaking changes to user-facing interfaces are unacceptable

This includes but is not limited to:

- All `komorebic` commands
- The `komorebi.json` schema
- The [
  `komorebi-application-specific-configuration`](https://github.com/LGUG2Z/komorebi-application-specific-configuration)
  schema

No user should ever find that their configuration file has stopped working after upgrading to a new version
of `komorebi`.

More often than not there are ways to reformulate changes that may initially seem like they require breaking user-facing
interfaces into additive changes.

For some inspiration please take a look
at [this commit](https://github.com/LGUG2Z/komorebi/commit/e7d928a065eb63bb4ea1fb864c69c1cae8cc763b) which added the
ability for users to specify colours in `komorebi.json` in Hex format alongside RGB.

There is also a process in place for graceful, non-breaking, deprecation of configuration options that are no longer
required.

# Development

If you use IntelliJ, you should enable the following settings to ensure that code generated by macros is recognised by
the IDE for completions and navigation:

- Set `Expand declarative macros`
  to `Use new engine` under "Settings > Langauges & Frameworks > Rust"
- Enable the following experimental features:
    - `org.rust.cargo.evaluate.build.scripts`
    - `org.rust.macros.proc`

# Logs and Debugging

Logs from `komorebi` will be appended to `%LOCALAPPDATA%/komorebi/komorebi.log`; this file is never rotated or
overwritten, so it will keep growing until it is deleted by the user.

Whenever running the `komorebic stop` command or sending a Ctrl-C signal to `komorebi` directly, the `komorebi` process
ensures that all hidden windows are restored before termination.

If however, you ever end up with windows that are hidden and cannot be restored, a list of window handles known
to `komorebi` are stored and continuously updated in `%LOCALAPPDATA%/komorebi//komorebi.hwnd.json`.

## Restoring Windows

Running `komorebic restore-windows` will read the list of window handles and forcibly restore them, regardless of
whether the main `komorebi` process is running.

## Panics and Deadlocks

If `komorebi` ever stops responding, it is most likely either due to either a panic or a deadlock. In the case of a
panic, this will be reported in the log. In the case of a deadlock, there will not be any errors in the log, but the
process and the log will appear frozen.

If you believe you have encountered a deadlock, you can compile `komorebi` with `--features deadlock_detection` and try
reproducing the deadlock again. This will check for deadlocks every 5 seconds in the background, and if a deadlock is
found, information about it will appear in the log which can be shared when opening an issue.

# Window Manager State and Integrations

The current state of the window manager can be queried using the `komorebic state` command, which returns a JSON
representation of the `State` struct.

This may also be polled to build further integrations and widgets on top of.

# Window Manager Event Subscriptions

## Named Pipes

It is possible to subscribe to notifications of every `WindowManagerEvent` and `SocketMessage` handled
by `komorebi` using [Named Pipes](https://docs.microsoft.com/en-us/windows/win32/ipc/named-pipes).

First, your application must create a named pipe. Once the named pipe has been created, run the following command:

```powershell
komorebic.exe subscribe-pipe <your pipe name>
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
in `komorebi::core`.

Below is an example of how you can subscribe to and filter on events using a named pipe in `nodejs`.

```javascript
const { exec } = require("child_process");
const net = require("net");

const pipeName = "\\\\.\\pipe\\komorebi-js";
const server = net.createServer((stream) => {
  console.log("Client connected");

  // Every time there is a workspace-related event, let's log the names of all
  // workspaces on the currently focused monitor, and then log the name of the
  // currently focused workspace on that monitor

  stream.on("data", (data) => {
    let json = JSON.parse(data.toString());
    let event = json.event;

    if (event.type.includes("Workspace")) {
      let monitors = json.state.monitors;
      let current_monitor = monitors.elements[monitors.focused];
      let workspaces = monitors.elements[monitors.focused].workspaces;
      let current_workspace = workspaces.elements[workspaces.focused];

      console.log(
        workspaces.elements
          .map((workspace) => workspace.name)
          .filter((name) => name !== null)
      );
      console.log(current_workspace.name);
    }
  });

  stream.on("end", () => {
    console.log("Client disconnected");
  });
});

server.listen(pipeName, () => {
  console.log("Named pipe server listening");
});

const command = "komorebic subscribe-pipe komorebi-js";

exec(command, (error, stdout, stderr) => {
  if (error) {
    console.error(`Error executing command: ${error}`);
    return;
  }
});
```

## Unix Domain Sockets

It is possible to subscribe to notifications of every `WindowManagerEvent` and `SocketMessage` handled
by `komorebi` using [Unix Domain Sockets](https://devblogs.microsoft.com/commandline/af_unix-comes-to-windows/).

UDS are also the only mode of communication between `komorebi` and `komorebic`.

First, your application must create a socket in `$ENV:LocalAppData\komorebi`. Once the socket has been created, run the
following command:

```powershell
komorebic.exe subscribe-socket <your socket name>
```

If the socket exists, komorebi will start pushing JSON data of successfully handled events and messages as in the
example above in the Named Pipes section.

## Rust Client

As of `v0.1.22` it is possible to use the `komorebi-client` crate to subscribe to notifications of
every `WindowManagerEvent` and `SocketMessage` handled by `komorebi` in a Rust codebase.

Below is a simple example of how to use `komorebi-client` in a basic Rust application.

```rust
// komorebi-client = { git = "https://github.com/LGUG2Z/komorebi", tag = "v0.1.38"}

use anyhow::Result;
use komorebi_client::Notification;
use komorebi_client::NotificationEvent;
use komorebi_client::UnixListener;
use komorebi_client::WindowManagerEvent;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;

pub fn main() -> anyhow::Result<()> {
  let socket = komorebi_client::subscribe(NAME)?;

  for incoming in socket.incoming() {
    match incoming {
      Ok(data) => {
        let reader = BufReader::new(data.try_clone()?);

        for line in reader.lines().flatten() {
          let notification: Notification = match serde_json::from_str(&line) {
            Ok(notification) => notification,
            Err(error) => {
              log::debug!("discarding malformed komorebi notification: {error}");
              continue;
            }
          };

          // match and filter on desired notifications
        }
      }
      Err(error) => {
        log::debug!("{error}");
      }
    }
  }

}
```

A read-world example can be found
in [komokana](https://github.com/LGUG2Z/komokana/blob/feature/komorebi-uds/src/main.rs).

## Subscription Event Notification Schema

A [JSON Schema](https://json-schema.org/) of the event notifications emitted to subscribers can be generated with
the `komorebic notification-schema` command. The output of this command can be redirected to the clipboard or a file,
which can be used with services such as [Quicktype](https://app.quicktype.io/) to generate type definitions in different
programming languages.

## Communication over TCP

A TCP listener can optionally be exposed on a port of your choosing with the `--tcp-port=N` flag. If this flag is not
provided to `komorebi` or `komorebic start`, no TCP listener will be created.

Once created, your client may send
any [SocketMessage](https://github.com/LGUG2Z/komorebi/blob/master/komorebi/src/core/mod.rs#L37) to `komorebi` in the
same way that `komorebic` would.

This can be used if you would like to create your own alternative to `komorebic` which incorporates scripting and
various middleware layers, and similarly it can be used if you would like to integrate `komorebi` with
a [custom input handler](https://github.com/LGUG2Z/komorebi/issues/176#issue-1302643961).

If a client sends an unrecognized message, it will be disconnected and have to reconnect before trying to communicate
again.

## Socket Message Schema

A [JSON Schema](https://json-schema.org/) of socket messages used to send instructions to `komorebi` can be generated
with the `komorebic socket-schema` command. The output of this command can be redirected to the clipboard or a file,
which can be used with services such as [Quicktype](https://app.quicktype.io/) to generate type definitions in different
programming languages.

# Appreciations

- First and foremost, thank you to my wife, both for naming this project and for her patience throughout its
  never-ending development

- Thank you to [@sitiom](https://github.com/sitiom) for
  being [an exemplary open source community leader](https://jeezy.substack.com/p/the-open-source-contributions-i-appreciate)

- Thank you to the developers of [nog](https://github.com/TimUntersberger/nog) who came before me and whose work taught
  me more than I can ever hope to repay

- Thank you to the developers of [GlazeWM](https://github.com/lars-berger/GlazeWM) for pushing the boundaries of tiling
  window management on Windows with me and having an excellent spirit of collaboration

- Thank you to [@Ciantic](https://github.com/Ciantic) for helping me bring
  the [hidden Virtual Desktops cloaking function](https://github.com/Ciantic/AltTabAccessor/issues/1) to `komorebi`
