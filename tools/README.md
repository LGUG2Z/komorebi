# tools/

Developer tools for the three-platform workspace (Windows, KDE, macOS). They are bash scripts that only read
the repository (and `jj`), write persistent cargo target directories outside it, and never commit or push.
Run them from anywhere inside a jj workspace of the repository.

Prerequisites: `jj`, `nix` with flakes (`nix develop`, `nix shell nixpkgs#gcc`), `jq`, `bash`, `flock` (only
for gate-lock). The Windows check runs through the dev shell of the root flake (`nix develop`, which provides
the xwin cross toolchain), so it needs no Windows host. `shellcheck` and `bash -n` are clean on every script.

| tool | what it does |
|---|---|
| `fast-check` | runs only the checks the touched paths need (tiers below) |
| `mac-check.sh` | the macOS type-check on Linux, unconditionally (the macOS step of `fast-check`) |
| `cc-fake.sh` | the fake C compiler `mac-check.sh` and `fast-check` use for `aarch64-apple-darwin` |
| `kde-closure-check` | fails when `komorebi-for-kde/nix/root-crates.json` and the KDE cargo closure differ |
| `e2e-needed` | says whether a diff touches anything the Windows e2e binaries are built from |
| `trim-target` | deletes stale test executables from the persistent target directories |
| `gate-lock` | a host-wide FIFO lock for heavy gates (optional) |
| `lite-env` | source it to build with debug info off (optional) |

## fast-check

`tools/fast-check [--list] [--range | --since REV]`. It reads `jj diff --name-only` of `@` (`--range`:
`trunk()..@`; `--since REV`: `REV..@`), runs only the checks the touched paths need, prints the time of each
check, and exits non-zero on failure. `--list` is a dry run. An empty `@` runs nothing: use `--since`.

The loops are tiered: tier 1 is the seconds-long cargo check and unit tests of the tree you edit, tier 2 the
type-check or clippy of the other platforms, and the nix builds (below) are the pre-landing gate, not part of
this tool.

| touched | check (tier) |
|---|---|
| `komorebi-for-kde/**` | `cargo check`, `cargo test --lib` and `--test schemas -- --test-threads=1` of the KDE daemon package (1) |
| `komorebi-for-mac/**` | `cargo check --target aarch64-apple-darwin --all-targets` with the fake CC; type-check only, cannot link or test (2) |
| root Windows tree (`komorebi*/`, `e2e/`, `Cargo.*`) | `nix develop --command cargo clippy --locked --all-targets -- -D warnings` (2) |
| `komorebi-layouts/`, `komorebi-scrolling/` | native `cargo test` of both plus the Windows check |
| `crates/**` | native tests, plus the KDE, macOS and Windows checks |
| `*.nix`, `flake.lock` | evaluation only: `nix flake show` and `nix eval ...drvPath` of the named checks (no `nix flake check`) |
| `*.md`, `docs/`, `tools/`, images, `.github` | nothing |

Persistent target directories live under `$FAST_CHECK_SCRATCH` (default `${XDG_CACHE_HOME:-~/.cache}/komorebi-fast-check`):
`target-fast-win` (2.8 GB), `kde-target` (shared, 30 GB, grows with test binaries: `tools/trim-target`),
`target-fast-mac` (2.2 GB), `target-fast-native` (0.5 GB). Delete them to reclaim disk; the next run is cold.

Measured warm times (2026-10-31, jj revision c89651fa, WSL2 host): Windows clippy 8-9 s, KDE check 1-3 s and
`--lib` 1.4 s, macOS 3 s (`-p komorebi`) / 2 s (workspace), native 2-3 s. On a busy host run it under
`nice -n 19` with `CARGO_BUILD_JOBS=2`.

Caveats

- Windows clippy here is NOT the pre-landing gate: still run `nix build .#checks.x86_64-linux.komorebi-workspace-clippy`
  (about 3.5 min) before landing; it uses crane's filtered source and catches filter bugs.
- macOS needs `COMMIT_HASH` (the tools set `dev`) when there is no `.git`.
- In a jj workspace without `.git`, `nix flake check --no-build` fails with `attribute 'dirtyRev' missing`, and it
  evaluates every check (15-16 GB). `fast-check` therefore evaluates attributes with `nix flake show` and `nix eval`.
- KDE `tests/schemas.rs` shares a pid-only socket path; keep `--test-threads=1`.
- Single KDE test file: `CARGO_TARGET_DIR=$FAST_CHECK_SCRATCH/kde-target nix shell nixpkgs#gcc -c cargo test -p komorebi-linux --test <name>`.

## macOS type-check (mac-check.sh, cc-fake.sh)

macOS cannot be built, linked or tested on Linux. `tools/mac-check.sh [cargo args]` runs
`cargo check --target aarch64-apple-darwin --all-targets` inside `nix shell nixpkgs#gcc nixpkgs#pkg-config nixpkgs#openssl.dev`
with `CC_aarch64_apple_darwin=tools/cc-fake.sh`. The `-sys` crates' build scripts call the C compiler for the
Darwin target, which does not exist here; `cc-fake.sh` turns every compile into an empty object (preprocessing
`-E` goes to the real `gcc`) so the build scripts "succeed" and rustc type-checks the macOS code. It proves the
code type-checks, nothing about behaviour. `fast-check` uses the same script when macOS files change.

## kde-closure-check

`tools/kde-closure-check [repo]` (2 s) compares `komorebi-for-kde/nix/root-crates.json` (the root crates the KDE
flake's source filter takes) with `cargo metadata` / `cargo tree` of the KDE packages. Run it after any
`Cargo.toml` dependency change in the KDE tree or in the crates it uses; it exits 1 and prints the differences
when the list is stale (a stale list breaks the KDE nix build or rebuilds the VM suites needlessly).

## e2e-needed

`tools/e2e-needed [--range | --since REV] [--to REV] [--closure]` answers whether a diff can change
`komorebi.exe`, `komorebic.exe` or `komorebi-testwin.exe` (the Windows e2e inputs): the source directories of the
workspace crates they depend on (`cargo tree --offline --target x86_64-pc-windows-msvc`, 1-2 s, no build), files
pulled in by `include_str!`/`include!`, fixed inputs (`Cargo.lock`, root `Cargo.toml`, `rust-toolchain.toml`,
`flake.nix`, `flake.lock`, build scripts) and the harness (`e2e/`). `NO` means a passing e2e of the previous
binaries carries over; `YES` lists the files that decided it; files it does not understand count as YES. It
does not cover nextest, clippy, the bar screenshot, the KDE VM or the schema oracles.

## trim-target

`tools/trim-target [MINUTES]` (default 45) removes test executables not touched for that long from the
persistent target directories (a debug test binary is about 290 MB; the KDE target grew from 30 GB to 109 GB in
one evening without this). They are rebuilt on demand.

## gate-lock (optional)

A host-wide FIFO lock for heavy gates so two of them never run at once and a timing-sensitive run (Windows
e2e, KDE `commands`/`core-e2e`) is not disturbed. Bash plus `flock`; state in `${XDG_STATE_HOME:-~/.local/state}/komorebi-gate`
(`GATE_LOCK_DIR` overrides).

```
tools/gate-lock run --class heavy  --label "clippy" --est 5 -- nix build -L --no-link .#checks.x86_64-linux.komorebi-workspace-clippy
tools/gate-lock run --class timing --label "win e2e" --est 60 --idle-load 3 -- nix run .#e2e-windows -- ...
tools/gate-lock run --class light  -- cargo check -p komorebi     # never waits; throttled while a timing gate is held
tools/gate-lock status [--short] | wait [--max SEC] | history [N] | adopt PID
```

`light` never waits (it runs `nice -n 19` with `CARGO_BUILD_JOBS=${GATE_LIGHT_JOBS:-3}` while a timing gate is
held), `heavy` and `timing` queue FIFO. It cannot stop work that does not use it and does not pin CPUs.

## lite-env (optional)

`. tools/lite-env` sets `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` (unless
already set) and `CARGO_BUILD_JOBS=3`: same code and tests, far smaller test binaries and less disk write.
Panic backtraces lose line numbers. Use a separate `CARGO_TARGET_DIR` (the first build is cold). Not for nix builds.
