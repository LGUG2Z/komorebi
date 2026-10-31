# Getting started

`komorebi` is a tiling window manager for macOS that is comprised of two
main binaries, `komorebi`, which contains the window manager itself,
and `komorebic`, which is the main way to send commands to the tiling
window manager.

It is important to note that neither `komorebi` nor `komorebic` handle
key bindings, because `komorebi` is a tiling window manager and not a hotkey
daemon.

This getting started guide suggests the installation of
[`skhd`](https://github.com/koekeishiya/skhd) to allow you to bind `komorebic`
commands to hotkeys to allow you to communicate with the tiling window manager
using keyboard shortcuts.

## Installation

Nightly builds are available to install
via [Homebrew](https://github.com/LGUG2Z/homebrew-tap), and you may also build
from source if you would prefer.

- [Homebrew](#homebrew)
- [Building from source](#building-from-source)

## System settings suggestions

Go to `System Settings -> Desktop & Dock -> Mission Control -> Displays have
separate Spaces` and disable this option if you are using more than one monitor.

`komorebi` is scoped to a single macOS space (the one that is active when the
process is started), which means that you can still use other Spaces as if
`komorebi` were not running at all. If you are using monitors with the above
option enabled, komorebi will only be listening to events from the Space on the
monitor which it was launched on.

Go to `System Settings -> Desktop & Dock -> Mission Control -> Group windows
by application` and disable this option if you like to use mission control.

## Homebrew

First add the `lgug2z/tap` Homebrew Tap

```shell
brew tap lgug2z/tap
```

Then install the `komorebi-for-mac-nightly` package

```shell
brew install lgug2z/tap/komorebi-for-mac-nightly
```

You may also optionally install the `skhd` package

```shell
brew install skhd
```

## Nix Flake

Ensure that your system Flake is configured to use your GitHub token so that you can add a private repository as an input

```nix
{
  nix = {
    settings = {
      access-tokens = [
        "github.com=YOUR ACCESS TOKEN"
      ];
    };
  };
}
```

Add the repository as an input

```nix
{
  inputs.komorebi-for-mac.url = "github:KomoCorp/komorebi-for-mac";
}
```

Add the overlay to your system's nixpkgs configuration

```nix
{
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ komorebi-for-mac.overlays.default ];
    # the rest of your pkgs configuration
  };
}
```

Add `pkgs.komorebi-full` to `environment.systemPackages` (you may also add
`pkgs.komorebi`, `pkgs.komorebic` and `pkgs.komorebi-bar` individually)

```nix
{
  environment.systemPackages = [
    pkgs.komorebi-full
  ];
}
```

## Building from source

Make sure you have installed [`rustup`](https://rustup.rs), and a stable `rust`
compiler toolchain.

Clone the git repository, enter the directory, and build the following binaries:

```powershell
cargo +stable install --path komorebi --locked --target-dir ~/.cargo/bin
cargo +stable install --path komorebic --locked --target-dir ~/.cargo/bin
cargo +stable install --path komorebi-bar --locked --target-dir ~/.cargo/bin
```

If the binaries have been built and added to your `$PATH` correctly, you should
see some output when running `komorebi --help` and `komorebic --help`

Once komorebi is installed, proceed to get
the [example configurations](example-configurations.md).

## Uninstallation

Before uninstalling, first run `komorebic stop` to make sure that
the `komorebi` processes have been stopped.

Finally, you can run the following commands to clean up files created by the
`quickstart` command and any other runtime files:

```powershell
rm -rf ~/.cargo/bin/komorebi
rm -rf ~/.cargo/bin/komorebic
rm -rf ~/.cargo/bin/komorebi-bar
rm -rf ~/.config/komorebi
rm -rf "~/Library/Application Support/komorebi"
```
