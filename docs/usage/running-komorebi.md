# Running komorebi

`komorebi` is the daemon that tiles your windows, and `komorebic` is the command line you
control it with. This page covers starting and stopping the daemon, asking it what it is
doing, and reading its log.

## Getting the two programs

[Installing komorebi](../installation.md) covers NixOS, where the flake's module installs both
and starts the daemon with every Plasma session, and building them from source. In a checkout of
the repository, on Linux, `nix build` leaves them in `result/bin`, and `nix profile install .`
puts them on your `PATH`. Without nix, build them with the Rust toolchain that
`rust-toolchain.toml` selects:

```sh
cargo build --release --locked
export PATH="$PWD/target/release:$PATH"
```

With the NixOS module, the daemon is the user unit `komorebi.service`, which starts when you log
in to Plasma. `systemctl --user start komorebi` starts it again after a `komorebic stop`, and
`journalctl --user -u komorebi` shows what it wrote. Everything else on this page applies to it
as it is.

## Starting and stopping

[`komorebic start`](../cli/start.md) runs the `komorebi` on your `PATH` in the background,
and checks three seconds later that it is running. It tries three times. If `komorebi` has
not started by then, `komorebic start` runs it once more in the foreground and prints what
it wrote on its standard error, which explains why it did not start. When a `komorebi` is
running already, `komorebic start` does nothing.

`komorebi` manages the windows of the Plasma Wayland session it is started in: it connects
to KWin on the session bus and loads a script into it, so start it from a terminal in that
session. Outside a Plasma session it exits with an error that says what is missing, which
`komorebic start` prints.

Everything after a `--` is passed to `komorebi`. `--fake-backend` runs it against an
in-memory desktop with one monitor and no windows instead of KWin, which is a way to try the
daemon and `komorebic` without a compositor:

```sh
komorebic start
komorebic start -- --log-level debug
komorebic start -- --fake-backend
```

[`komorebic stop`](../cli/stop.md) tells the daemon to stop. It brings back the windows it hid
(the back of a stack, [stacking windows](stacking-windows.md)), removes the desktop they were
hidden on, asks KWin to unload its script, removes its socket and exits. Stopping the process with
`SIGINT` or `SIGTERM` does the same.

## Asking komorebi what it is doing

The daemon answers questions on its socket, and these commands print the answers:

- [`komorebic state`](../cli/state.md): every monitor, with its workspaces, their
  containers and the windows in them, as JSON.
- [`komorebic monitor-information`](../cli/monitor-information.md): the id, the name and
  the bounds of every monitor, as JSON.
- [`komorebic query`](../cli/query.md): one value, the answer to one question: the index of
  the focused monitor, workspace, container or window, the layout of the focused workspace,
  whether the focused container is a single window or a stack, or the version of the
  daemon.

```sh
komorebic query focused-workspace-index
komorebic state | jq '.monitors.elements[0].workspaces.elements[0].containers.elements | length'
```

[`komorebic retile`](../cli/retile.md) tiles the windows of every monitor again, which puts
a window that was moved by hand back on its tile.

When the daemon is not running, these commands fail: there is no socket to write to.

A program that wants to know what komorebi does without asking can subscribe to its
notifications instead ([Subscribing to notifications](subscribing-to-notifications.md)).

## The socket and the data directory

The daemon listens on `komorebi.sock` in its data directory, `$XDG_RUNTIME_DIR/komorebi`
(the local data directory, `~/.local/share/komorebi`, when there is no runtime directory).
[`komorebic data-directory`](../cli/data-directory.md) prints the directory, if it exists.

## The log

The daemon logs to its standard output and to a file a day in `~/.local/share/komorebi`
(`$XDG_DATA_HOME/komorebi` when that is set): `komorebi.log.YYYY-MM-DD`, with colors, and
`komorebi_plaintext.log.YYYY-MM-DD`, without. The day is the UTC date.

[`komorebic log`](../cli/log.md) follows the file of the current day, with colors, until
you press Ctrl-C. The `--log-level` of `komorebi` sets how much is logged: `error`, `warn`,
`info` (the default), `debug` or `trace`.

## Shell completions

[`komorebic completions`](../cli/completions.md) prints a completion script for bash,
elvish, fish, PowerShell or zsh:

```sh
komorebic completions fish > ~/.config/fish/completions/komorebic.fish
komorebic completions bash > ~/.local/share/bash-completion/completions/komorebic
```

## Every command

[`docs/cli`](../cli/start.md) has a page for every `komorebic` command, generated from its
help with `just docgen`.
