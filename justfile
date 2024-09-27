set windows-shell := ["pwsh.exe", "-NoLogo", "-Command"]
export RUST_BACKTRACE := "full"

clean:
    cargo clean

fmt:
    cargo +nightly fmt
    cargo +stable clippy
    prettier --write README.md
    prettier --write .goreleaser.yml
    prettier --write .github/dependabot.yml
    prettier --write .github/FUNDING.yml
    prettier --write .github/workflows/windows.yaml

install-target target:
    cargo +stable install --path {{ target }} --locked

install:
    just install-target komorebic
    just install-target komorebic-no-console
    just install-target komorebi-gui
    just install-target komorebi-bar
    just install-target komorebi

run:
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
    cargo +stable run --bin komorebi --locked --features deadlock_detection

docgen:
    cargo run --package komorebic -- docgen
    Get-ChildItem -Path "docs/cli" -Recurse -File | ForEach-Object { (Get-Content $_.FullName) -replace 'Usage: ', 'Usage: komorebic.exe ' | Set-Content $_.FullName }

schemagen:
    cargo run --package komorebic -- static-config-schema > schema.json
    cargo run --package komorebic -- application-specific-configuration-schema > schema.asc.json
    cargo run --package komorebi-bar -- --schema > schema.bar.json
    generate-schema-doc .\schema.json --config template_name=js_offline --config minify=false .\static-config-docs\
