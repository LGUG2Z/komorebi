# Quickstart and autostart

## Quickstart

[`komorebic quickstart`](../cli/quickstart.md) writes three example files into the config home
(`~/.config/komorebi`, or `$KOMOREBI_CONFIG_HOME`), which it makes if it has to:

- `komorebi.json`, which is [`docs/linux/example-configurations.md`](../example-configurations.md):
  eight workspaces on the first monitor, each with another layout, and some padding;
- `applications.json`, which is
  [`docs/linux/usage/application-specific-configuration.md`](application-specific-configuration.md#the-file-komorebi-ships),
  the rules `komorebi.json` names for the applications that need them;
- `skhdrc`, which is [`docs/linux/skhdrc.sample`](../skhdrc.sample): the [key bindings](keybindings.md)
  of the sample, in the format of skhd, which the daemon and `komorebic hotkeys compile` read from
  there.

When a file is there already, `quickstart` asks before it writes over it, and leaves the file
alone unless the answer is `y` or `yes`. It prints the files it wrote, and then tells you to run
`komorebic start`.

```sh
komorebic quickstart
komorebic start
```

## Starting komorebi with Plasma

On NixOS, [the module](../installation.md#nixos) starts komorebi with every Plasma session.
Elsewhere, [`komorebic enable-autostart`](../cli/enable-autostart.md) writes an XDG autostart
entry, `~/.config/autostart/komorebi.desktop` (`$XDG_CONFIG_HOME/autostart`), which Plasma runs at
login, after its workspace has started:

```sh
komorebic enable-autostart
komorebic enable-autostart --config ~/mine.json
```

The entry runs `komorebi`, or `komorebi --config` with the file `--config` names, as the session
finds `komorebi` on its `PATH`. [`komorebic disable-autostart`](../cli/disable-autostart.md) takes
the entry away again, and says so when there is none.

Do not use both the module and the entry: the session bus gives its name to one komorebi, and the
second exits.

## Starting over with another file

[`komorebic replace-configuration`](../cli/replace-configuration.md) makes the running daemon start
over from another `komorebi.json`:

```sh
komorebic replace-configuration ~/work.json
```

The daemon makes its window manager again from the file, as it does at start: it brings back
the windows it had hidden, the back of a stack, so that none is out of sight, loads the monitors
and the windows that are open from KWin, gives the monitors the entries of the file and tiles
them, and from then on watches the new file and not the one it had. It is not the same as
changing the file the daemon watches, which [Configuration](configuration.md#changing-the-file)
describes: the windows that were in a workspace go where the new file puts them, not where they
were. A file that is not a `komorebi.json` is logged, and the daemon carries on as it was.
