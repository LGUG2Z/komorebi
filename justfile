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

# the macros the codebase has agreed not to use, the workflows, which actionlint checks (with
# shellcheck over their scripts), and the probe script, all with the tools of the nixpkgs flake.lock pins
lint:
    test -z "$(rg 'eyre!' --type rust .)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust .)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./komorebi/src)" || (echo "println! macro not allowed"; false)
    nix shell --inputs-from . nixpkgs#actionlint nixpkgs#shellcheck --command actionlint .github/workflows/*.yaml
    nix shell --inputs-from . nixpkgs#shellcheck --command shellcheck scripts/wsl2-probe.sh

# the JSON Schemas of komorebi.json and of applications.json, from the Rust types
jsonschema:
    cargo run --package komorebic --locked -- static-config-schema > schema.json
    cargo run --package komorebic --locked -- application-specific-configuration-schema > schema.asc.json

# the options of the module's `config`, from schema.json, with the hash of the schema they were
# made from in the first line (the check `schema-fresh` compares it); needs schemars-nixgen
# (`cargo install schemars-nixgen`), which the dev shell does not have
nixgen:
    schemars-nixgen schema.json -o nix/komorebi-options.nix --name komorebi --description "komorebi for KDE Plasma configuration"
    printf '# schema-sha256: %s\n' "$(nix hash file --type sha256 --base16 schema.json)" | cat - nix/komorebi-options.nix > nix/komorebi-options.nix.new
    mv nix/komorebi-options.nix.new nix/komorebi-options.nix
    nix fmt nix/komorebi-options.nix
    nix build -L --no-link ".#checks.$(nix eval --raw --impure --expr builtins.currentSystem).nix-options-validation"

# probe a NixOS-WSL2 machine for KVM, WSLg, a GPU and RDP (docs/dev/wsl2-probe.md): `just wsl2-probe --dry-run`
wsl2-probe *args:
    nix run .#wsl2-probe -- {{ args }}

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

# write the help of every komorebic subcommand to docs/cli, one page each
docgen:
    rm -f docs/cli/*.md
    cargo run --package komorebic --locked -- docgen --output docs/cli

run target *args:
    cargo run --bin {{ target }} --locked -- {{ args }}
