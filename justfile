export RUST_BACKTRACE := "full"

# format the Rust sources (needs a nightly rustfmt: rustfmt.toml uses unstable options)
fmt:
    cargo fmt

# everything a change has to pass before it is pushed
check: lint check-js
    cargo fmt --check
    cargo clippy --workspace --all-targets --locked -- -D warnings
    cargo test --workspace --locked
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked

# the KWin bridge, which is JavaScript: its types, checked by tsc from its JSDoc comments, and its
# tests, which node runs against a fake KWin (no package.json, no build step)
check-js:
    cd komorebi/src/kwin && tsc -p tsconfig.json
    node --test komorebi/src/kwin/bridge.test.js

# the macros the codebase has agreed not to use
lint:
    test -z "$(rg 'eyre!' --type rust .)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust .)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./komorebi/src)" || (echo "println! macro not allowed"; false)

# one VM test, on this machine: `just vm kwin`. Linux boots the guest under KVM through the
# flake check; a Mac builds the guest on a builder and runs the driver (see docs/dev/vm-tests.md)
vm name:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname)" = Darwin ]; then
        scripts/vm-mac {{ name }}
    else
        nix build -L .#checks.{{ arch() }}-linux.vm-{{ name }}
    fi

# every VM test, in the order of the flake
vm-all:
    #!/usr/bin/env bash
    set -euo pipefail
    system=$(nix eval --raw --impure --expr builtins.currentSystem)
    for name in $(nix eval --json ".#legacyPackages.$system.vm" --apply builtins.attrNames | jq -r '.[]'); do
        just vm "$name"
    done

run target *args:
    cargo run --bin {{ target }} --locked -- {{ args }}
