# Key bindings

komorebi reads key bindings from a file in the format of skhd (`skhdrc`), whkd (`whkdrc`) or
sxhkd (`sxhkdrc`), the hotkey daemons of komorebi on macOS and on Windows and of X11 window
managers, so an existing file can be used as it is. On Plasma under Wayland no program sees
the keys of another, so the keys are KDE's global shortcuts, in one of two ways:

- **Static shortcuts**: KDE runs the command of the key by itself, from a `.desktop` file. A
  static shortcut works when komorebi is not running and shows in System Settings, where it
  can be changed, but it is one combo, pressed, the same in every application, and KDE takes
  about 0.2 seconds to start its command.
- **The engine**: the komorebi daemon registers the keys with KDE and runs the command of each
  press itself, a few milliseconds after the key. It realizes what a static shortcut cannot:
  modes, chord chains, bindings that fire on release, bindings that do something else in each
  application, a blacklist and a pause binding. It works only while the daemon runs.

The mode decides which bindings go where:

| Mode | Static shortcuts | The engine |
|---|---|---|
| `engine` (the default) | none | every binding |
| `auto` | every binding that can be one, in a file with no modes, blacklist or pause binding | the others |
| `static` | every binding that can be one | none: the others are left out, with a warning |

[`skhdrc.sample`](../skhdrc.sample), [`whkdrc.sample`](../whkdrc.sample) and
[`sxhkdrc.sample`](../sxhkdrc.sample) are the same bindings in the three formats.

## On NixOS

The module's `keybindings` options take the file or its text, and do the rest: the static
shortcuts are installed in the system profile, where KDE finds them when you log in, and the
daemon is started with the file and the same mode.

```nix
services.komorebi = {
  enable = true;
  keybindings = {
    format = "skhd";
    file = ./skhdrc;
    mode = "auto";
  };
};
```

| Option | Default | What it does |
|---|---|---|
| `keybindings.format` | none | `"skhd"`, `"whkd"` or `"sxhkd"`; required with `text` or `file`. |
| `keybindings.text`, `keybindings.file` | none | The bindings, or the file they are in: exactly one of the two. skhd's `.load` is refused, since the file it reads would not be in the store. |
| `keybindings.mode` | `"engine"` | The mode, as in the table above. |
| `keybindings.chordTimeout` | `3000` | How long, in milliseconds, the engine waits for the next key of a chord chain. |
| `keybindings.layout` | `"us"` | The keyboard layout positional key names are read with; `"us"` is the only one. |
| `keybindings.strict` | `false` | Fail the build when something in the file is not realized as its own daemon realizes it. |
| `keybindings.appAliases` | `{ }` | Other names of an application, for the bindings that are for one application: `{ firefox = [ "firefox-esr" ]; }`. |

What `komorebic hotkeys compile` had to say about the file (its diagnostics, below) is in the
build log, and in `diagnostics.txt` of the `komorebi-keybindings` package.

## Without NixOS

[`komorebic hotkeys compile`](../cli/hotkeys-compile.md) writes the static shortcuts of a
file to `~/.local/share/kglobalaccel`, where KDE reads them at the next login (`--reload`
hands new ones over at once), and the daemon runs the engine for the rest when it is given
the same mode. Without a file name both read the first of `skhdrc`, `whkdrc` and `sxhkdrc` in
`~/.config/komorebi`, where [`komorebic quickstart`](quickstart-and-autostart.md#quickstart)
writes the sample `skhdrc`.

```sh
komorebic hotkeys compile --mode auto --reload
komorebic start -- --keybindings-mode auto
```

The daemon's options are `--keybindings FILE`, `--keybindings-format skhd|whkd|sxhkd` (when
the name does not end with one of those), `--keybindings-mode`, `--chord-timeout MS` and
`--app-alias NAME=ALIAS[,ALIAS]...`.
[`komorebic export-kde-shortcuts`](../cli/export-kde-shortcuts.md) writes the static
shortcuts of an skhdrc and leaves the rest out, for a setup without the engine.

## What is realized, and what is not

Everything the three formats bind keys with is realized, by a static shortcut or by the
engine, except what follows. `komorebic hotkeys compile --dry-run` prints a diagnostic for
each case the file has, with its line, and `--strict` (`keybindings.strict`) turns a warning
into a failure.

- **Approximated**: side-specific modifiers (`lalt`, `rwin`) are either side, since KDE does
  not tell them apart. A capturing mode (skhd's `@`) lets the keys it has no binding for reach
  the application. skhd's `->` and sxhkd's `~` do not pass the key on to the application. A
  key that is not the next of an sxhkd chain does not end the chain: only Escape and the chord
  timeout do. A release binding (sxhkd's `@`) fires when any key of its combo is released
  first. `.shell` is not followed: commands run through `sh -c`.
- **Not supported**: skhd's `fn` modifier, `.load`, `.alias`, key forwarding and key
  sequences; mouse buttons; keypad keys; the X modifiers KDE has no equivalent for (`hyper`,
  `meta`, `mode_switch`, `lock`, `mod2`, `mod3`, `mod5`, `any`).
- **Applications**: a binding for an application matches the executable, the Wayland app id
  (`org.kde.konsole`) or the desktop entry of the focused window, whatever the case. skhd
  and whkd name applications as macOS and Windows do, so a name that differs on KDE needs an
  alias.
- **Positional keys**: skhd's keycodes (`0x21`) and whkd's `oem_*` keys are read as on a US
  keyboard, and a shifted symbol is the one a US keyboard types (`alt + shift - 1` is
  `Alt+!`).

## When something goes wrong

- [`komorebic check`](../cli/check.md) reads the file as `hotkeys compile` does, and says how
  many bindings it has, which lines it cannot read, and whether the shortcut files KDE reads are
  the ones `hotkeys compile` would write now ([Checking the setup](checking-the-setup.md#the-key-bindings)).
- [`komorebic hotkeys state`](../cli/hotkeys-state.md) shows the keys komorebi has
  registered with KDE and what the engine is doing: its mode, the keys it holds, and whether
  a chord chain is being typed.
- The daemon's log says which file it read and how many bindings went where, and, at the
  debug level, every diagnostic, every mode it enters and every application that takes the
  focus.
- A key that another shortcut already has is not given to komorebi, and the daemon logs a
  warning that names it. The same binding as a static shortcut and in the engine is one such
  case: give `hotkeys compile` and the daemon the same mode.
- A daemon that was killed leaves its keys registered with KDE, doing nothing.
  [`komorebic hotkeys release`](../cli/hotkeys-release.md) gives them back (the NixOS
  module's unit runs it whenever the daemon stops), and so does the next start of the daemon.
- [`komorebic hotkeys reload`](../cli/hotkeys-reload.md) has the daemon read its file again
  after a change.
