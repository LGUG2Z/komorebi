export RUST_BACKTRACE := "full"

# format the Rust sources (needs a nightly rustfmt: rustfmt.toml uses unstable options)
fmt:
    cargo fmt

# everything a change has to pass before it is pushed
check: lint
    cargo fmt --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

# the macros the codebase has agreed not to use
lint:
    test -z "$(rg 'eyre!' --type rust .)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust .)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./komorebi/src)" || (echo "println! macro not allowed"; false)

run target *args:
    cargo run --bin {{ target }} --locked -- {{ args }}
