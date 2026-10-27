# komorebi for KDE

A tiling window manager for KDE Plasma on Wayland, in the family of
[komorebi](https://github.com/LGUG2Z/komorebi) for Windows and
[komorebi for Mac](https://github.com/LGUG2Z/komorebi-for-mac). The daemon
decides where each window goes, and KWin, the window manager of Plasma, does the
moving and the drawing.

## Running it locally

```sh
cargo run --bin komorebi --locked -- --version
```

The `rust-toolchain.toml` at the root selects the Rust toolchain.

## Crates

- [`komorebi`](komorebi): the daemon.
- [`version`](version): the version string every binary reports.

## Checks

`just check` runs the formatting check, clippy with warnings denied, the tests
and rustdoc with warnings denied. Formatting needs a nightly `rustfmt`, because
[`rustfmt.toml`](rustfmt.toml) uses an unstable option.

## Documentation

`cargo doc --workspace --no-deps --open` builds the API documentation.
[`CONTRIBUTING.md`](CONTRIBUTING.md) says how to work on the code.

## License

komorebi for KDE is under the [Komorebi License](LICENSE.md). Contributions
are accepted under the terms in [`CONTRIBUTING.md`](CONTRIBUTING.md).
