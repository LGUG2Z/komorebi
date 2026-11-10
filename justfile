set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

export RUST_BACKTRACE := "full"

# The conditional attributes ([windows], [macos], [linux], [unix]) enable an item only on that OS, so each of the
# variables and recipes below that differ by platform is written once per platform instead of branching inside.

# the platform of the schemas and docs that `jsonschema` and `docgen` write
[windows]
platform := "windows"

[macos]
platform := "macos"

[linux]
platform := "linux"

# the binaries that are built, installed and run on this platform, by package name
[windows]
targets := "komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts komorebi-recorder"

[macos]
targets := "komorebic komorebi komorebi-bar komorebi-recorder"

[linux]
targets := "komorebic komorebi komorebi-recorder"

# the packages the workspace-wide commands (fmt, fix) run on: a bare cargo command builds the Windows set, and the
# crates of the other platforms do not compile here
[windows]
packages := ""

[macos]
packages := "--workspace --exclude komorebi-windows --exclude komorebi-linux --exclude komorebi-gui --exclude komorebic-no-console --exclude komorebi-shortcuts --exclude komorebi-e2e-agent --exclude komorebi-testwin"

[linux]
packages := "-p schemars-nixgen -p schemars-docgen -p komorebi-recorder -p komorebi-linux -p komorebi -p komorebic -p komorebi-client -p komorebi-hotkeys -p komorebi-protocol -p komorebi-config -p komorebi-collections -p komorebi-core -p komorebi-backend -p komorebi-settings -p komorebi-transport -p komorebic-core -p komorebi-bar-core -p komorebi-layouts -p komorebi-scrolling"

# How cargo is invoked. Outside nix it is rustup's, which takes the +toolchain. Inside `nix develop` (IN_NIX_SHELL) the
# toolchain is the nix one: there is no rustup to take +toolchain, and its cargo already formats with the nightly
# rustfmt. The default shell also exports CARGO_BUILD_TARGET=x86_64-pc-windows-msvc for the Windows cross build,
# which every native recipe on macOS and Linux drops (`nix develop .#native` leaves it out to begin with).
[windows]
cargo_stable := "cargo +stable"

[windows]
cargo_nightly := "cargo +nightly"

[unix]
cargo_stable := "env -u CARGO_BUILD_TARGET cargo" + if env("IN_NIX_SHELL", "") == "" { " +stable" } else { "" }

[unix]
cargo_nightly := "env -u CARGO_BUILD_TARGET cargo" + if env("IN_NIX_SHELL", "") == "" { " +nightly" } else { "" }

clean:
    cargo clean

# format the Rust sources with the nightly rustfmt (tools/format-all does it for every file, as the nix checks do)
fmt:
    {{ cargo_nightly }} fmt {{ packages }}
    {{ cargo_stable }} clippy {{ packages }}

fix:
    {{ cargo_stable }} clippy {{ packages }} --fix --allow-dirty

# the macros the codebase has agreed not to use, the workflows (actionlint, shellcheck) and the probe script
lint:
    test -z "$(rg 'eyre!' --type rust .)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust .)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./crates/komorebi-linux/src ./crates/komorebi-macos/src)" || (echo "println! macro not allowed"; false)
    nix shell --inputs-from . nixpkgs#actionlint nixpkgs#shellcheck --command actionlint .github/workflows/*.yaml
    nix shell --inputs-from . nixpkgs#shellcheck --command shellcheck scripts/linux/wsl2-probe.sh

# tools/test-partition needs bash, cargo and jq
# the classification of the packages by where their tests run matches the workspace, and the checker catches a broken one
[unix]
test-partition-check:
    tools/test-partition check
    tools/test-partition --self-test

[windows]
test-partition-check:
    bash tools/test-partition check
    bash tools/test-partition --self-test

# The neutral packages run on Linux and macOS; `just test-windows-full` runs them all too.
# the windows and os_sensitive packages of tools/test-partition.json with nextest, as the Windows CI job does
[windows]
test-windows:
    $packages = (bash tools/test-partition packages windows) + ' ' + (bash tools/test-partition packages os_sensitive); cargo nextest run --locked --no-fail-fast @($packages -split ' ')

# The same commands as the test step of .github/workflows/windows.yaml before the partition; run it by hand on Windows.
# every test of the Windows workspace, the neutral packages and the recorder included
[windows]
test-windows-full:
    cargo test --workspace --exclude komorebi-recorder --exclude komorebi-linux --exclude komorebi-macos --exclude komorebi-hotkeys --exclude schemars-nixgen --exclude schemars-docgen
    cargo test -p komorebi-recorder

# the KWin bridge of the KDE daemon (JavaScript): its types by tsc, its tests by node against a fake KWin
check-js:
    cd crates/komorebi-linux/src/kwin && tsc -p tsconfig.json
    node --test crates/komorebi-linux/src/kwin/bridge.test.js

[unix]
install-targets *targets:
    for target in {{ targets }}; do just install-target $target; done

[windows]
install-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target $_ }

# fails when a target is not one of the binaries of this platform (the default of `ok` is evaluated on every call)
[private]
check-target target ok=(if target =~ "^(" + replace(targets, " ", "|") + ")$" { "" } else { error(target + " is not built on " + platform + ", the targets are: " + targets) }):

install-target target: (check-target target)
    {{ cargo_stable }} install --path {{ target }} --locked --no-default-features

[unix]
install-targets-with-jsonschema *targets:
    for target in {{ targets }}; do just install-target-with-jsonschema $target; done

[windows]
install-targets-with-jsonschema *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target-with-jsonschema $_ }

install-target-with-jsonschema target: (check-target target)
    {{ cargo_stable }} install --path {{ target }} --locked

install:
    just install-targets {{ targets }}

install-with-jsonschema:
    just install-targets-with-jsonschema {{ targets }}

[unix]
build-targets *targets:
    for target in {{ targets }}; do just build-target $target; done

[windows]
build-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just build-target $_ }

build-target target: (check-target target)
    {{ cargo_stable }} build --package {{ target }} --locked --release --no-default-features

build:
    just build-targets {{ targets }}

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
    just copy-targets {{ targets }}

# run a binary of this platform; every binary of its package is built first, as komorebi-recorder opens its viewer as the separate komorebi-recorder-gui
run target *args: (check-target target)
    {{ cargo_stable }} build --package {{ target }} --bins --locked --no-default-features
    {{ cargo_stable }} run --package {{ target }} --bin {{ target }} --locked --no-default-features -- {{ args }}

warn target $RUST_LOG="warn":
    just run {{ target }}

info target $RUST_LOG="info":
    just run {{ target }}

debug target $RUST_LOG="debug":
    just run {{ target }}

trace target $RUST_LOG="trace":
    just run {{ target }}

deadlock $RUST_LOG="trace":
    {{ cargo_stable }} run --package komorebi --bin komorebi --locked --no-default-features --features deadlock_detection

# the JSON Schemas of the platform this runs on, from the Rust types, into schemas/<platform>/
jsonschema: && jsonschema-bar
    {{ cargo_stable }} run --package komorebic --locked -- static-config-schema > schemas/{{ platform }}/schema.json
    {{ cargo_stable }} run --package komorebic --locked -- application-specific-configuration-schema > schemas/{{ platform }}/schema.asc.json

# Linux has no bar, so no schema of one
[linux]
[private]
jsonschema-bar:

[macos]
[private]
[windows]
jsonschema-bar:
    {{ cargo_stable }} run --package komorebi-bar --locked -- --schema > schemas/{{ platform }}/schema.bar.json

# the help of every komorebic subcommand of the platform this runs on, one page each: docs/cli (Windows),
# docs/linux/cli or docs/macos/cli
[windows]
docgen-local:
    {{ cargo_stable }} run --package komorebic --locked -- docgen --output docs/cli

[linux]
[macos]
docgen-local:
    {{ cargo_stable }} run --package komorebic --locked -- docgen --output docs/{{ platform }}/cli

# the docs site, in this repository
starlight := "starlight"

# schemars-docgen, built from this tree: the tool is a member of the workspace (crates/schemars-docgen)
schemars_docgen := cargo_stable + " run --package schemars-docgen --locked --"

# What it writes: the configuration and bar schema pages (schemars-docgen, from schemas/<platform>/), the page of every
# komorebic command (komorebic docgen) and the one komorebic page of the platform that the site makes from them.

# the reference pages of the docs site for this platform, in their final form, into starlight/
[macos]
[windows]
docgen:
    rm {{ starlight }}/src/data/cli/{{ platform }}/*.md
    {{ cargo_stable }} run --package komorebic --locked -- docgen --output {{ starlight }}/src/data/cli/{{ platform }}
    {{ schemars_docgen }} schemas/{{ platform }}/schema.json --output {{ starlight }}/src/content/docs/reference/komorebi-{{ platform }}.mdx --title "komorebi.json ({{ platform }})" --description "komorebi configuration schema reference for {{ platform }}"
    {{ schemars_docgen }} schemas/{{ platform }}/schema.bar.json --output {{ starlight }}/src/content/docs/reference/bar-{{ platform }}.mdx --title "komorebi.bar.json ({{ platform }})" --description "komorebi-bar configuration schema reference for {{ platform }}"
    node {{ starlight }}/scripts/generate-cli-reference.cjs

# the site has the pages of Windows and macOS only; the Linux documents are docs/linux
[linux]
docgen:
    @echo "the docs site has no pages for Linux: run this on Windows or macOS" && exit 1

# schemars-nixgen, built from this tree: the tool is a member of the workspace (crates/schemars-nixgen)
nixgen := cargo_stable + " run --package schemars-nixgen --locked --"

# the options of the NixOS module (KDE) and the darwin module (macOS), from the schemas; built from crates/schemars-nixgen
nixgen-linux:
    {{ nixgen }} schemas/linux/schema.json -o nix/linux/komorebi-options.nix --name komorebi --description "komorebi for KDE Plasma configuration"
    nix fmt nix/linux/komorebi-options.nix
    nix build -L --no-link ".#checks.$(nix eval --raw --impure --expr builtins.currentSystem).linux-nix-options-validation"

nixgen-macos:
    {{ nixgen }} schemas/macos/schema.json -o nix/macos/komorebi-options.nix --name komorebi --description "komorebi for Mac configuration"
    {{ nixgen }} schemas/macos/schema.bar.json -o nix/macos/komorebi-bar-options.nix --name komorebi-bar --description "komorebi for Mac bar configuration"
    nix fmt nix/macos/
    nix build -f nix/macos/tests/default.nix

version := `cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name == "komorebi") | .version'`

[linux]
[macos]
schemapub: schemapub-files
    npx wrangler pages deploy --project-name komorebi --branch main ./komorebi-schema

[windows]
schemapub: schemapub-files
    npx wrangler pages deploy --project-name komorebi --branch main .\komorebi-schema

[private]
[unix]
schemapub-files:
    rm -rf komorebi-schema
    mkdir -p komorebi-schema
    cp schemas/{{ platform }}/schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    {{ if platform == "linux" { "true" } else { "cp schemas/" + platform + "/schema.bar.json komorebi-schema/komorebi.bar." + version + ".schema.json" } }}

[private]
[windows]
schemapub-files:
    if (Test-Path komorebi-schema) { Remove-Item -Recurse -Force komorebi-schema }
    mkdir -Force komorebi-schema
    cp schemas/windows/schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    cp schemas/windows/schema.bar.json komorebi-schema/komorebi.bar.{{ version }}.schema.json

# udeps needs a nightly compiler: `cargo +nightly` outside nix, and inside it the cargo-udeps wrapper of the dev shell
# puts one on its PATH. It checks the packages of this platform, so run it on each platform.
depcheck:
    cargo outdated --depth 2
    {{ cargo_nightly }} udeps --quiet {{ packages }}

deps:
    cargo update
    just depgen

depgen:
    cargo deny check
    # the crates of this workspace are listed by name and version: their `path+file://` source is a path on the machine this ran on
    cargo deny list --format json | jq 'del(.unlicensed) | walk(if type == "string" then sub(" path\\+file://.*$"; "") else . end)' > dependencies.json

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

# record a bug repro: screen + komorebi log/events + OS trace, then query it with `komorebi-recorder at`
record *args:
    just run komorebi-recorder {{ args }}

# render the icon: komorebi-recorder/assets/watermelon.svg into the PNG the windows set and the ICO Windows embeds
icon:
    python3 komorebi-recorder/assets/generate.py

# cross-compile komorebi-recorder.exe and komorebi-recorder-gui.exe into ./result-windows and ./result-windows-gui
build-windows-recorder:
    nix build .#windows-komorebi-recorder --out-link result-windows
    nix build .#windows-komorebi-recorder-gui --out-link result-windows-gui
