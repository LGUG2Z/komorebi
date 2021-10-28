set shell := ["cmd.exe", "/C"]
export RUST_BACKTRACE := "full"

clean:
    cargo clean

fmt:
    cargo +nightly fmt
    cargo +nightly clippy
    prettier --write README.md

install-komorebic:
    cargo +stable install --path komorebic --locked

install-komorebi:
    cargo +stable install --path komorebic --locked

install:
    just install-komorebic
    just install-komorebi
    komorebic ahk-library
    cat '%USERPROFILE%\komorebic.lib.ahk' > komorebic.lib.sample.ahk

run:
    just install-komorebic
    cargo +stable run --bin komorebi --locked

warn $RUST_LOG="warn":
    just run

info $RUST_LOG="info":
    just run

debug $RUST_LOG="debug":
    just run

trace $RUST_LOG="trace":
    just run

deadlock $RUST_LOG="trace":
    just install-komorebic
    cargo +stable run --bin komorebi --locked --features deadlock_detection
