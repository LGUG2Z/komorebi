export RUST_BACKTRACE := "full"

# everything CI checks, in order; run it inside `nix develop` (direnv does)
check: fmt-check clippy check-windows test doc deny

# nightly rustfmt, which the devShell provides, and the macros this project does not allow
fmt-check:
    test -z "$(rg 'eyre!' --type rust)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust)" || (echo "dbg! macro not allowed"; false)
    cargo fmt --check

# format everything: Rust, TOML, Nix and this file
fmt:
    cargo clippy --workspace --all-targets --locked
    nix fmt

fix:
    cargo clippy --workspace --all-targets --fix --allow-dirty

clippy:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# type-check every `cfg(windows)` line, tests included, from a Mac (the Windows devShell targets x86_64-pc-windows-msvc)
check-windows:
    nix develop .#windows --command cargo clippy --workspace --all-targets --locked -- -D warnings

# lib, integration and doc tests; the ones that read komorebi's source need KOMOREBI_MAC_SRC, which `nix develop` sets
test:
    cargo test --workspace --locked

# rustdoc with its warnings as errors, for both hosts: links in `cfg(windows)` docs are only checked on Windows
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
    nix develop .#windows --command env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

deny:
    cargo deny check

clean:
    cargo clean

install:
    cargo install --path komorebi-recorder --locked

build:
    cargo build --package komorebi-recorder --locked --release

# the recorder opens the viewer as komorebi-recorder-gui, a separate executable beside it, which `cargo run` of
# the recorder alone would not rebuild: build both
run target:
    cargo build --bins --locked
    cargo run --bin {{ target }} --locked

# record a bug repro: screen + komorebi log/events + OS trace, then query it with `komorebi-recorder at`
record *args:
    cargo build --bins --locked
    cargo run --bin komorebi-recorder --locked -- {{ args }}

# cross-compile komorebi-recorder.exe and komorebi-recorder-gui.exe into ./result-windows
build-windows:
    nix build .#komorebi-recorder-windows --out-link result-windows

# render the icon: komorebi-recorder/assets/watermelon.svg into the PNG the windows set and the ICO Windows embeds
icon:
    python3 komorebi-recorder/assets/generate.py

deps:
    cargo update
    just depgen

depgen:
    cargo deny check
    cargo deny list --format json | jq 'del(.unlicensed)' > dependencies.json
