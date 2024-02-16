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

prepare:
    komorebic ahk-asc '~/.config/komorebi/applications.yaml'
    komorebic pwsh-asc '~/.config/komorebi/applications.yaml'
    cat '~/.config/komorebi/komorebi.generated.ps1' >komorebi.generated.ps1
    cat '~/.config/komorebi/komorebi.generated.ahk' >komorebi.generated.ahk

install:
    just install-target komorebic
    just install-target komorebic-no-console
    just install-target komorebi

run:
    just install-target komorebic
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

docgen:
    komorebic docgen
    Get-ChildItem -Path "docs/cli" -Recurse -File | ForEach-Object { (Get-Content $_.FullName) -replace 'Usage: ', 'Usage: komorebic.exe ' | Set-Content $_.FullName }

exampledocs:
    cp whkdrc.sample docs/whkdrc.sample
    cp komorebi.example.json docs/komorebi.example.json

schemagen:
    komorebic static-config-schema > schema.json
    generate-schema-doc .\schema.json --config template_name=js_offline --config minify=false .\static-config-docs\
