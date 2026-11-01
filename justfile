set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

export RUST_BACKTRACE := "full"

# The binaries are the same three packages on every platform; Windows has three more.
windows_targets := "komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts"
unix_targets := "komorebic komorebi komorebi-bar"
# the platform of the schemas and docs that `jsonschema` and `docgen` write
platform := if os() == "windows" { "windows" } else if os() == "macos" { "macos" } else { "linux" }

clean:
    cargo clean

# format the Rust sources with the nightly rustfmt (tools/format-all does it for every file, as the nix checks do)
fmt:
    cargo +nightly fmt
    cargo +stable clippy

fix:
    cargo clippy --fix --allow-dirty

# the macros the codebase has agreed not to use, the workflows (actionlint, shellcheck) and the probe script
lint:
    test -z "$(rg 'eyre!' --type rust .)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust .)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./komorebi-linux/src ./komorebi-macos/src)" || (echo "println! macro not allowed"; false)
    nix shell --inputs-from . nixpkgs#actionlint nixpkgs#shellcheck --command actionlint .github/workflows/*.yaml
    nix shell --inputs-from . nixpkgs#shellcheck --command shellcheck scripts/linux/wsl2-probe.sh

# the KWin bridge of the KDE daemon (JavaScript): its types by tsc, its tests by node against a fake KWin
check-js:
    cd komorebi-linux/src/kwin && tsc -p tsconfig.json
    node --test komorebi-linux/src/kwin/bridge.test.js

[unix]
install-targets *targets:
    for target in {{ targets }}; do just install-target $target; done

[windows]
install-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target $_ }

install-target target:
    cargo +stable install --path {{ target }} --locked --no-default-features

[unix]
install-targets-with-jsonschema *targets:
    for target in {{ targets }}; do just install-target-with-jsonschema $target; done

[windows]
install-targets-with-jsonschema *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target-with-jsonschema $_ }

install-target-with-jsonschema target:
    cargo +stable install --path {{ target }} --locked

[unix]
install:
    just install-targets {{ unix_targets }}

[windows]
install:
    just install-targets {{ windows_targets }}

[unix]
install-with-jsonschema:
    just install-targets-with-jsonschema {{ unix_targets }}

[windows]
install-with-jsonschema:
    just install-targets-with-jsonschema {{ windows_targets }}

[unix]
build-targets *targets:
    for target in {{ targets }}; do just build-target $target; done

[windows]
build-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just build-target $_ }

build-target target:
    cargo +stable build --package {{ target }} --locked --release --no-default-features

[unix]
build:
    just build-targets {{ unix_targets }}

[windows]
build:
    just build-targets {{ windows_targets }}

[windows]
copy-target target:
    cp .\target\release\{{ target }}.exe $Env:USERPROFILE\.cargo\bin

[windows]
copy-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just copy-target $_ }

[windows]
wpm target:
    just build-target {{ target }} && wpmctl stop {{ target }}; just copy-target {{ target }} && wpmctl start {{ target }}

[windows]
copy:
    just copy-targets {{ windows_targets }}

run target *args:
    cargo +stable run --bin {{ target }} --locked --no-default-features -- {{ args }}

warn target $RUST_LOG="warn":
    just run {{ target }}

info target $RUST_LOG="info":
    just run {{ target }}

debug target $RUST_LOG="debug":
    just run {{ target }}

trace target $RUST_LOG="trace":
    just run {{ target }}

deadlock $RUST_LOG="trace":
    cargo +stable run --bin komorebi --locked --no-default-features --features deadlock_detection

# the JSON Schemas of the platform this runs on, from the Rust types, into schemas/<platform>/
jsonschema:
    cargo run --package komorebic --locked -- static-config-schema > schemas/{{ platform }}/schema.json
    cargo run --package komorebic --locked -- application-specific-configuration-schema > schemas/{{ platform }}/schema.asc.json
    {{ if platform == "linux" { "true" } else { "cargo run --package komorebi-bar --locked -- --schema > schemas/" + platform + "/schema.bar.json" } }}

# the help of every komorebic subcommand of the platform this runs on, one page each: docs/cli (Windows),
# docs/linux/cli or docs/macos/cli
docgen-local:
    cargo run --package komorebic --locked -- docgen --output {{ if platform == "windows" { "docs/cli" } else { "docs/" + platform + "/cli" } }}

# the reference pages of the Windows or macOS site, from the schemas and the command line
docgen starlight:
    rm {{ starlight }}/src/data/cli/{{ platform }}/*.md
    cargo run --package komorebic -- docgen --output {{ starlight }}/src/data/cli/{{ platform }}
    schemars-docgen ./schemas/{{ platform }}/schema.json --output {{ starlight }}/src/content/docs/reference/komorebi-{{ platform }}.mdx --title "komorebi.json ({{ platform }})" --description "komorebi configuration schema reference for {{ platform }}"
    schemars-docgen ./schemas/{{ platform }}/schema.bar.json --output {{ starlight }}/src/content/docs/reference/bar-{{ platform }}.mdx --title "komorebi.bar.json ({{ platform }})" --description "komorebi-bar configuration schema reference for {{ platform }}"

# the options of the NixOS module (KDE) and the darwin module (macOS), from the schemas; needs schemars-nixgen
nixgen-linux:
    schemars-nixgen schemas/linux/schema.json -o nix/linux/komorebi-options.nix --name komorebi --description "komorebi for KDE Plasma configuration"
    nix fmt nix/linux/komorebi-options.nix
    nix build -L --no-link ".#checks.$(nix eval --raw --impure --expr builtins.currentSystem).linux-nix-options-validation"

nixgen-macos:
    schemars-nixgen schemas/macos/schema.json -o nix/macos/komorebi-options.nix --name komorebi --description "komorebi for Mac configuration"
    schemars-nixgen schemas/macos/schema.bar.json -o nix/macos/komorebi-bar-options.nix --name komorebi-bar --description "komorebi for Mac bar configuration"
    nix fmt nix/macos/
    nix build -f nix/macos/tests/default.nix

version := `cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name == "komorebi") | .version'`

[unix]
schemapub:
    rm -rf komorebi-schema
    mkdir -p komorebi-schema
    cp schemas/{{ platform }}/schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    cp schemas/{{ platform }}/schema.bar.json komorebi-schema/komorebi.bar.{{ version }}.schema.json
    npx wrangler pages deploy --project-name komorebi --branch main ./komorebi-schema

[windows]
schemapub:
    rm -Force komorebi-schema
    mkdir -Force komorebi-schema
    cp schemas/windows/schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    cp schemas/windows/schema.bar.json komorebi-schema/komorebi.bar.{{ version }}.schema.json
    npx wrangler pages deploy --project-name komorebi --branch main .\komorebi-schema

depcheck:
    cargo outdated --depth 2
    cargo +nightly udeps --quiet

deps:
    cargo update
    just depgen

depgen:
    cargo deny check
    cargo deny list --format json | jq 'del(.unlicensed)' > dependencies.json

[windows]
procdump:
    cargo build --bin komorebi
    .\procdump.exe -ma -e -x . .\target\debug\komorebi.exe

# probe a NixOS-WSL2 machine for KVM, WSLg, a GPU and RDP (docs/linux/dev/wsl2-probe.md): `just wsl2-probe --dry-run`
wsl2-probe *args:
    nix run .#wsl2-probe -- {{ args }}

# one KDE VM test on this machine: `just vm kwin` (docs/linux/dev/vm-tests.md)
vm name:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "$(uname)" = Darwin ]; then
        scripts/linux/vm-mac {{ name }}
    else
        nix build -L .#legacyPackages.{{ arch() }}-linux.vm.{{ name }}
    fi

# every KDE VM test, in the order of the flake
vm-all:
    #!/usr/bin/env bash
    set -euo pipefail
    system=$(nix eval --raw --impure --expr builtins.currentSystem)
    for name in $(nix eval --json ".#legacyPackages.$system.vm" --apply builtins.attrNames | jq -r '.[]'); do
        just vm "$name"
    done

# the demo guest in a virtual machine, with komorebi tiling a few windows (docs/linux/trying-it.md);
# `just run-kde --doctor` says what this machine is missing
run-kde *args:
    nix run .#run-kde -- {{ args }}
