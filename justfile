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

install-komorebic:
    cargo +stable install --path komorebic --locked

install-komorebi:
    cargo +stable install --path komorebi --locked

install:
    just install-komorebic
    just install-komorebi
    cat '~/.config/komorebi/komorebi.generated.ps1' > komorebi.generated.ps1

run:
    just install-komorebic
    cargo +stable run --bin komorebi --locked -- -a

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
