# komorebi for KDE

A tiling window manager for KDE Plasma on Wayland, in the family of
[komorebi](https://github.com/LGUG2Z/komorebi) for Windows and
[komorebi for Mac](https://github.com/LGUG2Z/komorebi-for-mac). The daemon
decides where each window goes, and KWin, the window manager of Plasma, does the
moving and the drawing.

## Running it locally

```sh
nix develop
cargo run --bin komorebi --locked -- --version
```

`nix develop` provides the Rust toolchain that [`rust-toolchain.toml`](rust-toolchain.toml)
selects, a nightly `rustfmt`, and the tools `just check` and the flake's checks
use. With [direnv](https://direnv.net), the [`.envrc`](.envrc) does it on
`cd`. On Linux, `nix run .#komorebi -- --version` builds and runs the daemon
without a checkout of the toolchain.

## Crates

- [`komorebi`](komorebi): the daemon.
- [`version`](version): the version string every binary reports.

## Checks

`just check` runs the formatting check, clippy with warnings denied, the tests
and rustdoc with warnings denied, inside `nix develop`. `nix flake check` runs
the same checks as derivations, plus `cargo deny`, the TOML and Nix formatting,
and the tests under `cargo nextest`, on Linux and on an Apple silicon Mac. A Mac
runs the tests against the fake desktop, since KWin only exists on Linux.
`nix fmt` formats every file the checks look at.

The VM tests boot NixOS with Plasma and run a real KWin. `just vm NAME` runs
one; [`docs/dev/vm-tests.md`](docs/dev/vm-tests.md) has the details, including
how to run them on a Mac.

## Documentation

`cargo doc --workspace --no-deps --open` builds the API documentation.
[`CONTRIBUTING.md`](CONTRIBUTING.md) says how to work on the code, and
[`docs/dev/vm-tests.md`](docs/dev/vm-tests.md) how the VM tests work.

## License

komorebi for KDE is under the [Komorebi License](LICENSE.md). Contributions
are accepted under the terms in [`CONTRIBUTING.md`](CONTRIBUTING.md).
