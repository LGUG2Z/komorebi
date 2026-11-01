# The flake

One `flake.nix` and one `flake.lock` at the repository root build, check and test the three platforms:
Windows (cross-built on Linux with xwin), KDE Plasma (native Linux) and macOS (native on an Apple machine).
The nested flakes of `komorebi-for-kde/` and `komorebi-for-mac/` and the `kfde-` and `kfm-` input prefixes are gone.

## Layout

| File | What it holds |
|---|---|
| `flake.nix` | the inputs and the list of modules; nothing else |
| `nix/common.nix` | the systems, `pkgs`, the formatter (treefmt), the pre-commit hook, the dev shells, the default overlay and the old-name aliases |
| `nix/lib.nix` | helpers: `repoFile` (a file of the repository as a store path of its own), `mkPkgs`, the workspace version, the systems of each platform, the manifest-only source |
| `nix/workspace-src.nix` | the source of one platform, cut from the one Cargo workspace (below) |
| `nix/windows/{build,flake-module}.nix` | the xwin SDK, the clang-cl environment, the crane derivations, the `windows-*` outputs, `e2e-windows` |
| `nix/linux/{build,flake-module}.nix` | the crane derivations of the KDE packages, the `linux-*` checks, the `vm-*` checks, the NixOS module, the Plasma guests |
| `nix/macos/{build,flake-module}.nix` | the crane derivations of the macOS packages, the `macos-*` checks, the darwin module |
| `nix/linux/*`, `nix/macos/*` | the NixOS module and options, the darwin module and options, the VM test suites, the KWin effects |

## Naming rule

The unprefixed names are the NATIVE build of the system: on Linux (`x86_64-linux`, `aarch64-linux`) they are
the KDE packages, on macOS (`aarch64-darwin`, `x86_64-darwin`) the macOS ones. The Windows cross-built outputs
are prefixed `windows-`. Checks are `<platform>-<check>` (`windows-clippy`, `linux-nextest`, `macos-deny`), and
the VM tests keep their `vm-NAME` names.

| System | Windows (`windows-*`) | Linux (native names, `linux-*` checks, `vm-*`) | macOS (native names, `macos-*` checks) |
|---|---|---|---|
| `x86_64-linux` | packages, apps, checks, `e2e-windows` | packages, apps, checks, VM tests | |
| `aarch64-linux` | packages, apps, checks | packages, apps, checks, VM tests | |
| `aarch64-darwin` | packages, apps, checks | checks and `legacyPackages.vm` (the fake desktop and the guests' definitions) | packages, apps, checks |
| `x86_64-darwin` | | | packages, apps, checks |

## Old name -> new name

The names of the root flake before this change (the Windows flake, plus the nested flakes re-exported with the
prefixes `kfde-` and `kfm-`) and of the nested flakes themselves.

### Packages and apps

| Old | New |
|---|---|
| `packages.<s>.komorebi`, `komorebic`, `komorebic-no-console`, `komorebi-bar`, `komorebi-gui`, `komorebi-shortcuts`, `komorebi-testwin`, `komorebi-e2e-agent` (Windows) | `windows-komorebi`, `windows-komorebic`, `windows-komorebic-no-console`, `windows-komorebi-bar`, `windows-komorebi-gui`, `windows-komorebi-shortcuts`, `windows-komorebi-testwin`, `windows-komorebi-e2e-agent` |
| `windowsSdk` | `windows-sdk` |
| `komorebi-testwin-e2e` | `windows-komorebi-testwin-e2e` |
| `komorebi-full` (Windows) | `windows-komorebi-full` |
| `default` (Windows), `apps.default` | `windows-default` (`default` is now the native package) |
| `apps.komorebi`, `komorebic`, `komorebic-no-console`, `komorebi-bar`, `komorebi-gui`, `komorebi-shortcuts` (Windows) | `apps.windows-komorebi`, ... (`e2e-windows` is unchanged) |
| `kfde-komorebi`, `kfde-komorebic`, `kfde-komorebi-full`, `kfde-default` | `komorebi`, `komorebic`, `komorebi-full`, `default` on the Linux systems |
| `kfde-kde-rounded-corners-komorebi`, `kfde-kwin-effect-geometry-change-komorebi`, `kfde-run-kde`, `kfde-wsl2-probe`, `kfde-wsl2-probe-guest` | the same names without `kfde-` |
| `kfm-komorebi`, `kfm-komorebic`, `kfm-komorebi-bar`, `kfm-komorebi-full`, `kfm-default` | `komorebi`, `komorebic`, `komorebi-bar`, `komorebi-full`, `default` on the Darwin systems |
| apps of the nested flakes: `komorebi`, `komorebic`, `run-kde`, `wsl2-probe`, `default` (Linux); `komorebi`, `komorebic`, `komorebi-bar`, `default` (macOS) | unchanged, per system |

### Checks

| Old | New |
|---|---|
| `komorebi-workspace-clippy`, `-fmt`, `-toml-fmt`, `-deny`, `-nextest` (Windows) | `windows-clippy`, `windows-fmt`, `windows-toml-fmt`, `windows-deny` (`-nextest` ran no test and is gone, see flake-checks.md) |
| `e2e-tests` | `windows-e2e-tests` |
| `kfde-komorebi-workspace-clippy`, `-doc`, `-fmt`, `-toml-fmt`, `-deny`, `-nextest` | `linux-clippy`, `linux-doc`, `linux-fmt`, `linux-toml-fmt`, `linux-deny`, `linux-nextest` |
| `kfde-komorebi-bridge-js` | `linux-bridge-js` |
| `kfde-schema-fresh`, `kfde-nix-options-validation`, `kfde-keybindings-fixtures`, `kfde-nixos-module-eval`, `kfde-plasma-guests-eval` | `linux-schema-fresh`, `linux-nix-options-validation`, `linux-keybindings-fixtures`, `linux-nixos-module-eval`, `linux-plasma-guests-eval` |
| `kfde-wsl2-probe`, `kfde-wsl2-probe-dry-run`, `kfde-run-kde`, `kfde-run-kde-doctor`, `kfde-run-kde-dry-run` | `linux-wsl2-probe`, `linux-wsl2-probe-dry-run`, `linux-run-kde`, `linux-run-kde-doctor`, `linux-run-kde-dry-run` |
| `kfde-kde-rounded-corners-komorebi-install`, `kfde-kwin-effect-geometry-change-komorebi-install` | `linux-kde-rounded-corners-komorebi-install`, `linux-kwin-effect-geometry-change-komorebi-install` |
| `vm-NAME`, `vm-lib` (nested KDE flake only; the root flake left them out) | unchanged names, in `checks` (`vm-lib`) or the sets `vm-fast`, `vm-slow` (see Checks and flake-checks.md); the suites are also `legacyPackages.<s>.vm.NAME` |
| `kfm-komorebi-workspace-clippy`, `-fmt`, `-toml-fmt`, `-deny`, `-nextest` | `macos-clippy`, `macos-fmt`, `macos-toml-fmt`, `macos-deny`, `macos-nextest` |
| `kfm-nix-options-validation`, `kfm-darwin-module-validation` | `macos-nix-options-validation`, `macos-darwin-module-validation` |
| `pre-commit`, `treefmt`, `kfde-pre-commit`, `kfde-treefmt`, `kfm-pre-commit`, `kfm-treefmt` | `pre-commit`, `treefmt` (one each, for the whole repository) |

### Other outputs

| Old | New |
|---|---|
| `legacyPackages.<s>.kfde-vm` (root); `legacyPackages.<s>.vm`, `vmTiers`, `vmGuests`, `vmTcg` (nested) | `legacyPackages.<s>.vm`, `vmTiers`, `vmGuests`, `vmTcg` |
| `nixosModules.komorebi-for-kde` (root); `nixosModules.default`, `komorebi`, `forVmTests` (nested) | `nixosModules.komorebi` (= `default`), `forVmTests`; `komorebi-for-kde` stays as an alias |
| `darwinModules.komorebi-for-mac` (root); `darwinModules.default`, `komorebi` (nested) | `darwinModules.komorebi` (= `default`); `komorebi-for-mac` stays as an alias |
| `overlays.komorebi-for-kde` (root); `overlays.default`, `komorebi`, `forVmTests` (nested) | `overlays.default` and `komorebi` (the native packages of the system the overlay is applied to), `overlays.linux`, `overlays.macos`, `overlays.forVmTests`; `komorebi-for-kde` stays as an alias |
| `nixosConfigurations.*` (nested KDE flake) | unchanged: `plasma-vm`, `plasma-vm-session`, `plasma-utm`, `plasma-vm-x86_64`, ..., `run-kde-x86_64`, `run-kde-aarch64` |
| `devShells.<s>.default` | `devShells.<s>.default` (all three platforms, cross environment for Windows) and `devShells.<s>.native` (the same without the cross environment) |
| `formatter.<s>` | `formatter.<s>` (one treefmt for the repository) |

## Inputs

One of each: `nixpkgs` (nixpkgs-unstable), `flake-parts`, `crane`, `rust-overlay`, `treefmt-nix`,
`git-hooks-nix`, and `asc` (the application-specific configuration komorebic embeds on Windows). They are the
revisions the KDE flake was locked to, because the KDE VM suites run against the Plasma and KWin of the pinned
nixpkgs and the KDE flake was the newest of the three (nixpkgs f05535a0, 2026-10-27, KWin 6.8.2, which is
`testedMax` in `nix/linux/supported-kwin.nix`; rust-overlay 19ecdd08 gives rustc 1.99.1, and sysinfo 0.39.6
needs 1.95).

One second nixpkgs, `nixpkgs-x86_64-darwin` (16c7794d, 2026-01-02), is used for the `x86_64-darwin` outputs and
for nothing else: nixpkgs 26.11 dropped x86_64-darwin and evaluating it with the unified pin throws
(`Nixpkgs 26.11 has dropped support for x86_64-darwin`). Drop the input together with x86_64-darwin when Intel
Macs are no longer supported.

Updating: `nix flake update NAME` for one input; never a blanket `nix flake update`. A new nixpkgs has to keep
every VM suite green (`just vm-all`), and the Windows and macOS groups build.

## The source of a build

The Cargo workspace lists the members of all three platforms (`default-members` is the Windows set), so cargo
loads every member in every build. `nix/workspace-src.nix` cuts one source per platform:

- `real` is the filtered source: the platform's members, the root manifest, lock and toolchain files, the
  platform's own files of the crates the platforms share (`komorebic`, `komorebi-client`, `komorebi-bar`,
  `komorebi-daemon`: the `linux`, `macos` and `windows` modules and tests are told apart by name) and the data
  files the sources and tests read (docs, schemas, fixtures).
- `src` is `real` plus a stub for every other member: its manifest with an empty library and a `main` that does
  nothing. Stubs are needed: cargo refuses a workspace with a member that is not there, and `--locked` refuses
  a lock file that has packages the workspace does not have. A stub carries the manifest only, so an edit of
  another platform's code changes nothing here.
- The dependency derivations (`buildDepsOnly`) use a dummy source made of the manifests of every member
  (`manifestSource` in `nix/lib.nix`), the same for the three platforms. It is a plain path, not a
  derivation, so crane reads it at evaluation without building: `nix flake check --no-build --all-systems`
  needs that to evaluate the Darwin outputs on a Linux host.

The Linux source takes the crates listed in `nix/linux/root-crates.json` (the crates outside the Linux-only
ones that `cargo tree` says the KDE packages depend on, so an edit of the egui bar does not rebuild the KDE
packages); `tools/kde-closure-check` fails when the list and `cargo tree` differ. The Windows source is the
whole workspace except the Linux and macOS crates and files; the macOS source is the macOS crates, the shared
crates and `crates/`.

## Checks

Every platform's checks run on its own source, so a change in one platform's directories does not rebuild the
others' derivations. A change in a crate under `crates/` (or in `komorebic`, `komorebi-client`,
`komorebi-daemon`), in `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` or `deny.toml` rebuilds those that read it.

- `windows-clippy`, `windows-fmt`, `windows-toml-fmt`, `windows-deny`, `windows-e2e-tests`
- `linux-clippy`, `linux-doc`, `linux-nextest`, `linux-fmt`, `linux-toml-fmt`, `linux-deny`, `linux-bridge-js`,
  `linux-schema-fresh`, `linux-nix-options-validation`, `linux-keybindings-fixtures`, `linux-nixos-module-eval`,
  `linux-plasma-guests-eval`, `linux-wsl2-probe`, `linux-wsl2-probe-dry-run`, `linux-run-kde*`,
  `linux-*-install`
- `macos-clippy`, `macos-nextest`, `macos-fmt`, `macos-toml-fmt`, `macos-deny`, `macos-nix-options-validation`,
  `macos-darwin-module-validation`
- `treefmt` and `pre-commit` for the whole tree

The checks are in sets (docs/dev/flake-checks.md): `checks.<system>` has the members of the sets of
`defaultCheckSets` in `nix/checks.nix`, the cheap ones. The ones that evaluate NixOS systems (the VM suites,
`linux-nixos-module-eval`, `linux-plasma-guests-eval`, `linux-run-kde*`) are the sets `vm-fast`, `vm-slow`,
`linux-nixos-eval` and `linux-demo`, which `nix flake check` leaves out (evaluating them took more memory than the host
has): `nix build -L .#checkSets.<set>`, or one member, `.#checkMembers.<set>.<name>`. A check is named as before
(`nix build -L .#checks.x86_64-linux.windows-clippy`). `nix flake check --no-build --all-systems` evaluates the
default sets and every other output of every system and is the evaluation test of this file.

## Dev shells

`nix develop` has the tools of all three platforms: the nightly rustfmt over the toolchain of
`rust-toolchain.toml` with the Windows target, the xwin cross environment (`CARGO_BUILD_TARGET` is
`x86_64-pc-windows-msvc`, so a bare `cargo` command builds for Windows), cargo-deny, cargo-nextest, the KDE
bridge's node and tsc, the VM library's ruff and ty, QEMU and swtpm for the Windows e2e guest. For `cargo`
commands on the Linux or macOS packages use `nix develop .#native` (the same tools, no cross environment), or
`--target` on a command.

## Updating the flake: what to run

- the evaluation: `nix flake check --no-build --all-systems`
- the Windows group: `windows-clippy`, `windows-fmt`, `windows-toml-fmt`, `windows-deny`,
  `windows-e2e-tests`, the `windows-*` packages
- the Linux group: the `linux-*` checks and packages, one VM suite (`vm-lib`, then a cheap suite)
- on a Mac: `nix build -L .#checks.aarch64-darwin.macos-{clippy,nextest,fmt,toml-fmt,deny,nix-options-validation,darwin-module-validation}`
  and `nix build -L .#packages.aarch64-darwin.{komorebi,komorebic,komorebi-bar}`
