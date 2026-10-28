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
- It defines a systemd user unit, `komorebi.service`, for every user of the machine. Plasma's
  `plasma-workspace.target` starts it when a user logs in to a Plasma session, after
  `plasma-core.target`, which Plasma orders after KWin. The unit runs the daemon with the `PATH`
  of a terminal in the session.
- systemd starts the daemon again when it fails or is killed. When it is stopped (`komorebic
  stop`, or `systemctl --user stop komorebi`), it stays stopped until the next login or
  `systemctl --user start komorebi`.
- Without `services.desktopManager.plasma6.enable`, the configuration does not evaluate: an
  assertion of the module fails, and its message names the option to set.

### Options

| Option | Default | What it does |
|---|---|---|
| `services.komorebi.enable` | `false` | Installs komorebi and starts it with every Plasma session. |
| `services.komorebi.package` | `pkgs.komorebi-full` | The package with `bin/komorebi` and `bin/komorebic`; the unit runs the first, and both are installed. |
| `services.komorebi.logLevel` | `"info"` | The daemon's `--log-level`: `"error"`, `"warn"`, `"info"`, `"debug"` or `"trace"`. |
| `services.komorebi.configFile` | `null` | A `komorebi.json` for the daemon. The unit passes it to komorebi as `--config`, so the daemon reads it and not the `komorebi.json` of `~/.config/komorebi`. The file is in the Nix store: komorebi reads a new one when it starts again. |
| `services.komorebi.execStart` | read only | The command line the unit starts komorebi with, built from `package`, `logLevel` and `configFile`. A unit of your own that runs komorebi uses it to start komorebi the same way. |

The file for `configFile` is written like any other `komorebi.json`
([Configuration](usage/configuration.md)):

```nix
{
  services.komorebi = {
    enable = true;
    configFile = ./komorebi.json;
  };
}
```

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
`default`. To install them without the unit, and start komorebi yourself:

```sh
nix profile install github:LGUG2Z/komorebi-for-kde
```

On a NixOS configuration, the overlay, `overlays.default`, provides `pkgs.komorebi`,
`pkgs.komorebic` and `pkgs.komorebi-full` for `environment.systemPackages`.

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

To start it with every Plasma session instead, write the unit the NixOS module defines, without
the NixOS paths, to `~/.config/systemd/user/komorebi.service`:

```ini
[Unit]
Description=komorebi, the tiling window manager
After=plasma-core.target

[Service]
ExecStart=%h/.cargo/bin/komorebi
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
