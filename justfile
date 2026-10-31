set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]

export RUST_BACKTRACE := "full"

clean:
    cargo clean

fmt:
    cargo +nightly fmt
    cargo +stable clippy
    prettier --write .github/ISSUE_TEMPLATE/bug_report.yml
    prettier --write .github/ISSUE_TEMPLATE/config.yml
    prettier --write .github/ISSUE_TEMPLATE/feature_request.yml
    prettier --write .github/dependabot.yml
    prettier --write .github/FUNDING.yml
    prettier --write .github/workflows/windows.yaml

fix:
    cargo clippy --fix --allow-dirty

install-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target $_ }

install-target target:
    cargo +stable install --path {{ target }} --locked --no-default-features

install-targets-with-jsonschema *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target-with-jsonschema $_ }

install-target-with-jsonschema target:
    cargo +stable install --path {{ target }} --locked

install:
    just install-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts

install-with-jsonschema:
    just install-targets-with-jsonschema komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts

build-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just build-target $_ }

build-target target:
    cargo +stable build --package {{ target }} --locked --release --no-default-features

build:
    just build-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts

copy-target target:
    cp .\target\release\{{ target }}.exe $Env:USERPROFILE\.cargo\bin

copy-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just copy-target $_ }

wpm target:
    just build-target {{ target }} && wpmctl stop {{ target }}; just copy-target {{ target }} && wpmctl start {{ target }}

copy:
    just copy-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui komorebi-shortcuts

run target:
    cargo +stable run --bin {{ target }} --locked --no-default-features

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

docgen starlight:
    rm {{ starlight }}/src/data/cli/windows/*.md
    cargo run --package komorebic -- docgen --output {{ starlight }}/src/data/cli/windows
    schemars-docgen ./schema.json --output {{ starlight }}/src/content/docs/reference/komorebi-windows.mdx --title "komorebi.json (Windows)" --description "komorebi for Windows configuration schema reference"
    schemars-docgen ./schema.bar.json --output {{ starlight }}/src/content/docs/reference/bar-windows.mdx --title "komorebi.bar.json (Windows)" --description "komorebi-bar for Windows configuration schema reference"

jsonschema:
    cargo run --package komorebic -- static-config-schema > schema.json
    cargo run --package komorebic -- application-specific-configuration-schema > schema.asc.json
    cargo run --package komorebi-bar -- --schema > schema.bar.json

version := `cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.name == "komorebi") | .version'`

schemapub:
    rm -Force komorebi-schema
    mkdir -Force komorebi-schema
    cp schema.json komorebi-schema/komorebi.{{ version }}.schema.json
    cp schema.bar.json komorebi-schema/komorebi.bar.{{ version }}.schema.json
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

procdump:
    cargo build --bin komorebi
    .\procdump.exe -ma -e -x . .\target\debug\komorebi.exe
