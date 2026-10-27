export RUST_BACKTRACE := "full"

# everything CI checks, in order
check: fmt-check clippy test doc deny

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


# lib, integration and doc tests; the ones that read komorebi's source need KOMOREBI_MAC_SRC, which `nix develop` sets
test:
    cargo test --workspace --locked

# rustdoc with its warnings as errors, for both hosts: links in `cfg(windows)` docs are only checked on Windows
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

deny:
    cargo deny check

clean:
    cargo clean

install:
    cargo install --path komorebi-recorder --locked

build:
    cargo build --package komorebi-recorder --locked --release

run target:
    cargo run --bin {{ target }} --locked

# record a bug repro: screen + komorebi log/events + OS trace, then query it with `komorebi-recorder at`
record *args:
    cargo run --bin komorebi-recorder --locked -- {{ args }}


deps:
    cargo update
    just depgen

depgen:
    cargo deny check
    cargo deny list --format json | jq 'del(.unlicensed)' > dependencies.json
