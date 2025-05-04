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

install-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target $_ }

install-target target:
    cargo +stable install --path {{ target }} --locked --no-default-features

install-targets-with-jsonschema *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just install-target-with-jsonschema $_ }

install-target-with-jsonschema target:
    cargo +stable install --path {{ target }} --locked

install:
    just install-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui

install-with-jsonschema:
    just install-targets-with-jsonschema komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui

build-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just build-target $_ }

build-target target:
    cargo +stable build --package {{ target }} --locked --release --no-default-features

build:
    just build-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui

copy-target target:
    cp .\target\release\{{ target }}.exe $Env:USERPROFILE\.cargo\bin

copy-targets *targets:
    "{{ targets }}" -split ' ' | ForEach-Object { just copy-target $_ }

wpm target:
    just build-target {{ target }} && wpmctl stop {{ target }}; just copy-target {{ target }} && wpmctl start {{ target }}

copy:
    just copy-targets komorebic komorebic-no-console komorebi komorebi-bar komorebi-gui

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

docgen:
    cargo run --package komorebic -- docgen
    Get-ChildItem -Path "docs/cli" -Recurse -File | ForEach-Object { (Get-Content $_.FullName) -replace 'Usage: ', 'Usage: komorebic.exe ' | Set-Content $_.FullName }

jsonschema:
    cargo run --package komorebic -- static-config-schema > schema.json
    cargo run --package komorebic -- application-specific-configuration-schema > schema.asc.json
    cargo run --package komorebi-bar -- --schema > schema.bar.json

# this part is run in a nix shell because python is a nightmare
schemagen:
    rm -rf static-config-docs bar-config-docs
    mkdir -p static-config-docs bar-config-docs
    generate-schema-doc ./schema.json --config template_name=js_offline --config minify=false ./static-config-docs/
    generate-schema-doc ./schema.bar.json --config template_name=js_offline --config minify=false ./bar-config-docs/
    mv ./bar-config-docs/schema.bar.html ./bar-config-docs/schema.html

depgen:
    cargo deny list --format json | jq 'del(.unlicensed)' > dependencies.json
