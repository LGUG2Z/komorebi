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
cargo run --bin komorebi --locked -- --fake-backend
```

`--fake-backend` runs the window manager against an in-memory desktop with one
monitor and no windows, and logs what it does until it is stopped with Ctrl-C.
Without a backend, `komorebi` exits with a usage error.

`nix develop` provides the Rust toolchain that [`rust-toolchain.toml`](rust-toolchain.toml)
selects, a nightly `rustfmt`, and the tools `just check` and the flake's checks
use. With [direnv](https://direnv.net), the [`.envrc`](.envrc) does it on
`cd`. On Linux, `nix run .#komorebi -- --version` builds and runs the daemon
without a checkout of the toolchain.

## Crates

- [`komorebi`](komorebi): the daemon, and the library behind it. The library has
  the vocabulary the window manager is written in (`core`), the `Backend` trait
  through which it reaches a compositor and the in-memory desktop the tests use
  (`backend`), the model of what it tiles (`monitor`, `workspace`, `container`,
  `window`), the window manager that keeps the model (`window_manager`), loads
  it from the backend (`desktop`) and changes it when the compositor reports an
  event (`window_manager_event`, `process_event`), the `Ring` that holds
  monitors, workspaces and containers, and the lists in which some elements are
  locked in place.
- [`version`](version): the version string every binary reports.

The layouts, which turn a work area and a number of containers into a rectangle
for each, are `komorebi-layouts` from [komorebi](https://github.com/LGUG2Z/komorebi),
a git dependency at the revision `Cargo.toml` pins.

## Checks

`just check` runs the formatting check, clippy with warnings denied, the tests
and rustdoc with warnings denied, inside `nix develop`. `nix flake check` runs
the same checks as derivations, plus `cargo deny`, the TOML and Nix formatting,
and the tests under `cargo nextest`, on Linux and on an Apple silicon Mac. On
Linux it also boots every VM test, which needs KVM. A Mac
runs the tests against the fake desktop, since KWin only exists on Linux.
`nix fmt` formats every file the checks look at.

The VM tests boot NixOS with Plasma and run a real KWin. `just vm NAME` runs
one; [`docs/dev/vm-tests.md`](docs/dev/vm-tests.md) has the details, including
how to run them on a Mac.

## How it fits together

The daemon decides where every window goes and asks the compositor to put it
there. Everything it needs from the compositor is behind one trait, `Backend`,
so the window manager logic runs against a real compositor and against an
in-memory fake desktop alike. [`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md)
describes the trait and where each of its rules is enforced.

The window manager models the desktop as monitors, each with a ring of
workspaces, each with a ring of containers, each with a ring of windows. A
workspace's layout gives each of its containers a tile of the monitor's work
area, and every window of a container is moved onto its tile. At startup the
window manager loads the monitors and the windows that are open from the
backend and tiles them; after that it changes the model when the compositor
reports that a window was shown, closed, focused, minimized or restored, and
tiles the workspace in front again.

## Documentation

`cargo doc --workspace --no-deps --open` builds the API documentation.
[`CONTRIBUTING.md`](CONTRIBUTING.md) says how to work on the code.
[`docs/dev/ARCHITECTURE.md`](docs/dev/ARCHITECTURE.md) describes the model,
[`docs/dev/DESIGN.md`](docs/dev/DESIGN.md) says why it is built that way, and
[`docs/dev/vm-tests.md`](docs/dev/vm-tests.md) how the VM tests work.

## License

komorebi for KDE is under the [Komorebi License](LICENSE.md). Contributions
are accepted under the terms in [`CONTRIBUTING.md`](CONTRIBUTING.md).
