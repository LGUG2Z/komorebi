export RUST_BACKTRACE := "full"

fmt:
    test -z "$(rg 'eyre!' --type rust)" || (echo "eyre! macro not allowed"; false)
    test -z "$(rg 'dbg!' --type rust)" || (echo "dbg! macro not allowed"; false)
    test -z "$(rg 'println!' --type rust ./komorebi/src)" || (echo "println! macro not allowed"; false)
    cargo clippy
    prettier --write README.md
    nix fmt

fix:
    cargo clippy --fix --allow-dirty

clean:
    cargo clean

install-targets *targets:
    for target in {{ targets }}; do just install-target $target; done

install-target target:
    cargo install --path {{ target }} --locked --no-default-features

install-targets-with-jsonschema *targets:
    for target in {{ targets }}; do just install-target-with-jsonschema $target; done

install-target-with-jsonschema target:
    cargo install --path {{ target }} --locked

install:
    just install-targets komorebic komorebi komorebi-bar

install-with-jsonschema:
    just install-targets-with-jsonschema komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts

build-targets *targets:
    for target in {{ targets }}; do just build-target $target; done

build-target target:
    cargo build --package {{ target }} --locked --release --no-default-features

build:
    just build-targets komorebic komorebi komorebi-bar

run target:
    cargo run --bin {{ target }} --locked

error target $RUST_LOG="komorebi=error":
    just run {{ target }}

warn target $RUST_LOG="komorebi=warn":
    just run {{ target }}

info target $RUST_LOG="komorebi=info":
    just run {{ target }}

debug target $RUST_LOG="komorebi=debug":
    just run {{ target }}

trace target $RUST_LOG="komorebi=trace":
    just run {{ target }}

deadlock $RUST_LOG="trace":
    cargo run --bin komorebi --locked --no-default-features --features deadlock_detection

docgen starlight:
    rm {{ starlight }}/src/data/cli/macos/*.md
    cargo run --package komorebic -- docgen --output {{ starlight }}/src/data/cli/macos
    schemars-docgen ./schema.json --output {{ starlight }}/src/content/docs/reference/komorebi-macos.mdx --title "komorebi.json (macOS)" --description "komorebi for Mac configuration schema reference"
    schemars-docgen ./schema.bar.json --output {{ starlight }}/src/content/docs/reference/bar-macos.mdx --title "komorebi.bar.json (macOS)" --description "komorebi-bar for Mac configuration schema reference"

jsonschema:
    cargo run --package komorebic -- static-config-schema > schema.json
    cargo run --package komorebic -- application-specific-configuration-schema > schema.asc.json
    cargo run --package komorebi-bar -- --schema > schema.bar.json

nixgen:
    schemars-nixgen schema.json -o nix/komorebi-options.nix --name komorebi --description "komorebi for Mac configuration"
    schemars-nixgen schema.bar.json -o nix/komorebi-bar-options.nix --name komorebi-bar --description "komorebi for Mac bar configuration"
    nix fmt nix/
    nix build -f nix/tests/default.nix

version := `cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name == "komorebi") | .version'`

schemapub:
    rm -rf komorebi-schema
    mkdir -p komorebi-schema
    cp schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    cp schema.bar.json komorebi-schema/komorebi.bar.{{ version }}.schema.json
    wrangler pages deploy --project-name komorebi-for-mac --branch main ./komorebi-schema

depcheck:
    cargo outdated --depth 2
    cargo udeps --quiet

deps:
    cargo update
    just depgen

depgen:
    cargo deny check
    cargo deny list --format json | jq 'del(.unlicensed)' > dependencies.json

push:
    git push origin master
    git push komocorp master
