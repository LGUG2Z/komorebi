# Installing komorebi

`komorebi` is two programs: `komorebi`, the daemon that tiles the windows, and `komorebic`, the
command line that controls it. The daemon runs in a KDE Plasma Wayland session with KWin 6.8 or
newer ([PLATFORM.md](dev/PLATFORM.md) has what it needs from the session).

On NixOS, the repository's flake has a module that installs both programs and starts komorebi
with every Plasma session. On any other distribution, build them from source.

There is no binary cache and there are no prebuilt binaries: the Komorebi license does not allow
third parties to redistribute binaries, so every installation builds komorebi from source. On
NixOS, Nix does that for you, the first time you switch to a configuration that has it.

## NixOS

### The module

Add the repository to the inputs of your system's flake, and import its module,
`nixosModules.default`, into your configuration:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    komorebi.url = "github:LGUG2Z/komorebi-for-kde";
  };

  outputs =
    { nixpkgs, komorebi, ... }:
    {
      nixosConfigurations.HOSTNAME = nixpkgs.lib.nixosSystem {
        modules = [
          komorebi.nixosModules.default
          ./configuration.nix
        ];
      };
    };
}
```

Then enable it, next to Plasma, in `configuration.nix`:

```nix
{
  services.desktopManager.plasma6.enable = true;
  services.komorebi.enable = true;
}
```

The module adds the flake's overlay, which provides `pkgs.komorebi-full`, to your nixpkgs, so
nothing else is needed. komorebi is built with the flake's nixpkgs and the Rust toolchain that
the repository's `rust-toolchain.toml` selects, whatever nixpkgs your system uses.

### What the module does

- It installs `komorebi` and `komorebic` in the system profile, so they are on every user's
  `PATH`.
- It installs KDE Rounded Corners, the KWin effect that draws komorebi's borders, unless
  `services.komorebi.effects.borders.enable` is `false`, and Geometry Change, the effect that
  animates windows, unless `services.komorebi.effects.animation.enable` is `false`. KWin finds
  effects when it starts, so a session that was running when you switched to the new
  configuration has them after the next login.
- It defines a systemd user unit, `komorebi.service`, for every user of the machine. Plasma's
  `plasma-workspace.target` starts it when a user logs in to a Plasma session, after
  `plasma-core.target`, which Plasma orders after KWin. The unit runs the daemon with the `PATH`
  of a terminal in the session.
- systemd starts the daemon again when it fails or is killed. When it is stopped (`komorebic
  stop`, or `systemctl --user stop komorebi`), it stays stopped until the next login or
  `systemctl --user start komorebi`.
- After the daemon stops, however it stops, the unit runs `komorebic hotkeys release`, which
  gives back to KDE the keys the daemon had registered for its key bindings: a daemon that was
  killed could not, and KDE would keep them, doing nothing, until the next start.
- Without `services.desktopManager.plasma6.enable`, the configuration does not evaluate: an
  assertion of the module fails, and its message names the option to set.

### Options

| Option | Default | What it does |
|---|---|---|
| `services.komorebi.enable` | `false` | Installs komorebi and starts it with every Plasma session. |
| `services.komorebi.package` | `pkgs.komorebi-full` | The package with `bin/komorebi` and `bin/komorebic`; the unit runs the first, and both are installed. |
| `services.komorebi.logLevel` | `"info"` | The daemon's `--log-level`: `"error"`, `"warn"`, `"info"`, `"debug"` or `"trace"`. |
| `services.komorebi.config` | `null` | The configuration, as Nix: every key of `komorebi.json`, typed from the schema. The module writes it as a `komorebi.json`, checks that against the schema, and the unit passes it to komorebi as `--config`. Ignored if `configFile` is set. |
| `services.komorebi.configFile` | `null` | A `komorebi.json` for the daemon, checked against the schema when the system is built. The unit passes it to komorebi as `--config`, so the daemon reads it and not the `komorebi.json` of `~/.config/komorebi`. The file is in the Nix store: komorebi reads a new one when it starts again. |
| `services.komorebi.applicationSpecificConfiguration` | `enable = true`, `sources = [ ]` | Files of [application specific configuration](usage/application-specific-configuration.md) fetched from GitHub (`owner`, `repo`, `rev`, `path` and `hash` each), which become the `app_specific_configuration_path` of the configuration `config` makes. There are none by default. |
| `services.komorebi.effects.borders.enable` | `true` | Installs the KWin effect that draws komorebi's borders. Whether borders are drawn is komorebi's own setting ([Borders](usage/borders.md)). KWin finds effects when it starts: log in again after the first switch. The module warns when `config` sets `border` to `true` and this is off. |
| `services.komorebi.effects.borders.package` | `pkgs.kde-rounded-corners-komorebi` | The effect: KDE Rounded Corners 0.11.0, built against the KWin of the system and not loaded until komorebi asks. |
| `services.komorebi.effects.animation.enable` | `true` | Installs the KWin effect that animates komorebi's windows going to their places. Whether windows are animated is komorebi's own setting ([Animation](usage/animation.md)). KWin finds effects when it starts: log in again after the first switch. |
| `services.komorebi.effects.animation.package` | `pkgs.kwin-effect-geometry-change-komorebi` | The effect: Geometry Change, a scripted effect that is not loaded until komorebi asks. |
| `services.komorebi.execStart` | read only | The command line the unit starts komorebi with, built from `package`, `logLevel`, `configFile` or `config`, and `keybindings`. A unit of your own that runs komorebi uses it to start komorebi the same way. |
| `services.komorebi.keybindings.format` | `null` | `"skhd"`, `"whkd"` or `"sxhkd"`: the format of the key bindings. Required with `text` or `file`. |
| `services.komorebi.keybindings.text`, `.file` | `null` | The key bindings, or the file they are in: exactly one of the two. |
| `services.komorebi.keybindings.mode` | `"engine"` | Which bindings are static shortcuts, installed in the system profile, and which the daemon runs: `"engine"`, `"auto"` or `"static"`. |
| `services.komorebi.keybindings.chordTimeout` | `3000` | How long, in milliseconds, the daemon waits for the next key of a chord chain. |
| `services.komorebi.keybindings.layout` | `"us"` | The keyboard layout of positional key names; `"us"` is the only one. |
| `services.komorebi.keybindings.strict` | `false` | Fail the build when something in the key bindings is not realized as its own hotkey daemon realizes it. |
| `services.komorebi.keybindings.appAliases` | `{ }` | Other names of an application, for the bindings that are for one application. |

[Key bindings](usage/keybindings.md) explains the key binding options.

`config` takes the keys of `komorebi.json` ([Configuration](usage/configuration.md)) as Nix, and a
value of the wrong kind, or a layout that does not exist, is an error when the configuration is
evaluated:

```nix
{
  services.komorebi = {
    enable = true;
    config = {
      default_workspace_padding = 15;
      monitors = [
        {
          workspaces = [
            { name = "web"; layout = "BSP"; }
            { name = "code"; layout = "Columns"; }
          ];
        }
      ];
    };
  };
}
```

`configFile` takes a `komorebi.json` you wrote:

```nix
{
  services.komorebi.configFile = ./komorebi.json;
}
```

Comment lines (starting with `//`) are taken out of the file before it is checked, which is why
the file can have them. With both, `configFile` wins, and the module warns that `config` is
ignored.

### Running it

`plasma-workspace.target` starts komorebi when you log in. In a session that was already
running when you switched to the new configuration, start it yourself:

```sh
systemctl --user start komorebi
systemctl --user status komorebi
journalctl --user -u komorebi
```

[`komorebic stop`](cli/stop.md) stops it until you start it again or log in again.
[`komorebic start`](cli/start.md) finds the daemon the unit runs and does nothing, and a second
`komorebi` in the same session exits: the session bus gives the name komorebi needs to one
daemon. [Running komorebi](usage/running-komorebi.md) has the rest: asking komorebi what it is
doing, and its log.

### The packages without the module

The flake's packages are `komorebi`, `komorebic`, and `komorebi-full`, which has both and is the
`default`, `kde-rounded-corners-komorebi`, the effect that draws the borders, and
`kwin-effect-geometry-change-komorebi`, the effect that animates windows. To install them without the unit, and start komorebi yourself:

```sh
nix profile install github:LGUG2Z/komorebi-for-kde
```

On a NixOS configuration, the overlay, `overlays.default`, provides `pkgs.komorebi`,
`pkgs.komorebic`, `pkgs.komorebi-full`, `pkgs.kde-rounded-corners-komorebi` and
`pkgs.kwin-effect-geometry-change-komorebi` for `environment.systemPackages`.

## Other distributions: from source

You need a Plasma 6.8 (or newer) Wayland session, [rustup](https://rustup.rs) with the stable
toolchain, which `rust-toolchain.toml` selects, and a C compiler (`cc`): the crate that makes the
version string builds libgit2, and zlib when the system has none. Then:

```sh
git clone https://github.com/LGUG2Z/komorebi-for-kde
cd komorebi-for-kde
cargo install --locked --path komorebi
cargo install --locked --path komorebic
```

`cargo` installs both in `~/.cargo/bin`, which has to be on your `PATH`. Start komorebi from a
terminal in your Plasma session with [`komorebic start`](cli/start.md).

To start it with every Plasma session instead, `komorebic enable-autostart` writes an autostart
entry ([Quickstart and autostart](usage/quickstart-and-autostart.md)), or you can write the unit
the NixOS module defines, without the NixOS paths, to `~/.config/systemd/user/komorebi.service`:

```ini
[Unit]
Description=komorebi, the tiling window manager
After=plasma-core.target

[Service]
ExecStart=%h/.cargo/bin/komorebi
ExecStopPost=%h/.cargo/bin/komorebic hotkeys release
Restart=on-failure
SuccessExitStatus=130

[Install]
WantedBy=plasma-workspace.target
```

and enable it with `systemctl --user enable komorebi`. At time of writing (2026-10-27), only the
module's unit has been run, on NixOS; this one has not been tried on another distribution.

## Uninstalling

Stop the daemon first, with `komorebic stop`.

- With the module, remove `services.komorebi.enable` (or the module) from your configuration and
  switch to it.
- From source, remove the unit if you wrote one (`systemctl --user disable komorebi`, then the
  file), and the programs, with `cargo uninstall komorebi komorebic`.

The daemon's logs are in `~/.local/share/komorebi` (`$XDG_DATA_HOME/komorebi`), which you can
delete. Its socket is in `$XDG_RUNTIME_DIR/komorebi`, which is removed when you log out.
