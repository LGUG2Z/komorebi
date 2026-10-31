# One `komorebi`, one `komorebic`, one `komorebi-bar`

Goal: three packages and three binaries that build on Windows, KDE (Linux) and macOS, the platform
chosen by `cfg(target_os)` (generics, `cfg_select!`, exhaustive `match`; no macros as a design tool).
This file records a spike (throwaway revisions on bookmark `wip-u-pkg-spike`, titled `spike:`; nothing
landed on `wip-main`), the layout it recommends, the migration order, the risks and the gates.

Measured on `wip-main` 633e7453 with the light gates only (`nice -n 19`, 3 jobs, persistent target
dirs, `COMMIT_HASH=dev`); no nix build, no e2e, no VM. macOS is type-checked with the fake CC only: it
was never linked or run.

## What the spike did

1. `spike: one root workspace ...` (rev `yuuyzssl`): the 10 KDE and macOS member crates became members
   of the root workspace. Their two `[workspace]` manifests were deleted, their `workspace = true`
   dependencies were written out, packages renamed (`komorebi-linux`, `komorebic-linux`,
   `komorebi-client-linux`, `komorebi-hotkeys-linux`, `version-linux`, and `-macos` for the macOS
   ones including `komorebi-bar-macos`); binary and lib names stayed (`[lib] name`, `[[bin]] name`) so
   no source file changed. One `Cargo.lock`.
2. `spike: unified komorebic package by #[path] includes`: the root `komorebic` package became the
   unified one. `src/main.rs` is a `cfg_select!` over `target_os` that includes the old Windows
   `main.rs` (moved to `windows.rs`) and, by `#[path]`, the KDE and macOS `main.rs` where they live.
   Dependencies are in `[target.'cfg(...)'.dependencies]` tables. Edits to the old files: `fn main`
   became `pub fn main`; `crate::HOME_DIR` became `super::HOME_DIR` in KDE `hotkeys.rs`.
3. `spike: target-gate the macOS daemon crate`: objc2 and friends under
   `[target.'cfg(target_os = "macos")'.dependencies]` plus `#![cfg(target_os = "macos")]` in
   `komorebi-macos`, to test `cargo check --workspace` on a foreign target.

The script that did step 1 is `S/pkg-spike.py` (scratchpad; rerunnable on a fresh `wip-main`).

## Results

| question | result |
|---|---|
| One lock resolves? | Yes. 859 packages (root) -> 900. All 859 Windows entries unchanged. The KDE lock had 644 and the macOS lock 786 entries, so +41 only. Crates with more than one version: 81 -> 95 (the `sysinfo` 0.38/0.39/0.40, `dirs` 6/7, `parking_lot` 0.12/0.13, `shadow-rs` 1/2, `netdev` 0.40/0.46 splits coexist; nothing needs unifying). |
| Locked versions kept? | Not exactly: crates that only KDE or macOS use were re-resolved to newest: `zbus`/`zbus_macros` 6.1 -> 6.2, `zbus_utils`, `zruntime`, `sysinfo` 0.40.0 -> 0.40.1, `libz-sys`, `unicode-width`. Pin with `cargo update zbus@6.2.0 --precise 6.1.0` (needs the `name@version` spelling) if the VM suites must see the old versions. |
| Windows `cargo check -p komorebic --target x86_64-pc-windows-msvc` (nix develop) | passes, 14.8 s wall warm (11.2 s compile); unified `komorebic` also passes, 7 s. |
| KDE native `cargo check -p komorebi-linux -p komorebic-linux` | passes; 1 m 38 s cold into a fresh lite target dir. Unified `komorebic` on Linux passes (1.4 s after touch) when openssl is available (see risk 2). |
| KDE tests unchanged | `cargo test -p komorebi-linux --lib`: 294 passed. `--test schemas -- --test-threads=1`: 5 passed. All four KDE packages, `--no-fail-fast`, 1 m 26 s warm: 879 passed, 1 failed. The failure is `tests/version.rs`: `komorebi-linux 0.1.0` instead of `komorebi 0.1.0` (clap takes the command name from `CARGO_PKG_NAME`); a renamed package needs `#[command(name = "komorebi")]`. Nothing else depends on the package name. |
| macOS fake-CC `cargo check --target aarch64-apple-darwin -p komorebi-macos -p komorebic-macos -p komorebi-bar-macos --all-targets --locked` | passes, 32 s warm (6 warnings in the bar, as before). Unified `komorebic` on macOS passes, 12 s. Type-check only. |
| Feature unification | Resolver 2 keeps `komorebi-layouts/darwin` off Windows: `cargo tree -p komorebic --target x86_64-pc-windows-msvc -e features -i komorebi-layouts` shows only `default`, the macOS package shows `darwin`. This holds only while the package that asks for `darwin` is not in the same build; `cargo ... --workspace` on one target selects them all (see risk 3). |
| `cargo check --workspace` for Windows (what clippy, fmt, nextest and crane do today) | FAILS in 52 s: `objc2 only works on Apple platforms`. With objc2 target-gated and `#![cfg(target_os = "macos")]`, `komorebi-macos` compiles empty on Windows (spike 3), so the fix is to gate every foreign-platform crate, not to exclude them. Done for one crate only. |
| Same dependency key, different path per target | REJECTED by cargo: `Dependency 'komorebi-client' has different source paths depending on the build target`. Same for `version`. So the unified package must use one key per platform (`komorebi-client-linux`, `komorebi-client-macos`, ...) and the platform module must alias it (`extern crate komorebi_client_linux as komorebi_client;` at the root, or `use komorebi_linux as komorebi;` at the top of each platform module). The spike used `extern crate`. |
| Compile time | No extra cost on a target: a build selects its own target's dependency tables, so Windows never sees zbus or objc2 and Linux never sees `windows`. The only added cost is resolution (`cargo metadata` 6.5 s with the network). |

## Option A or B

Measured size of what would have to move into `#[cfg(target_os)] mod` of one crate (Option B):
daemon `crate::` paths: Windows 669, KDE 447, macOS 587 (35,149 / 32,932 / 34,505 lines, 3 lib+bin
crates each with their own tests and a `komorebi-client` that re-exports them); bar 111 / 90 (7,710 /
5,551 lines); `komorebic` 0 (1,467 / 807 / 1,537 lines).

Recommendation:

- `komorebic`: B. One package `komorebic`, `src/main.rs` = `cfg_select!` choosing `mod windows`,
  `mod linux`, `mod macos` (each the present `main.rs`, `hotkeys.rs` for Linux), one clap command name,
  target-gated dependencies. Zero `crate::` rewrites, so the files move whole. This is what the spike
  built (with `#[path]` instead of the final moves).
- `komorebi-bar`: B, as `bar-unification.md` already plans (`src/bar/{mod,windows,macos}.rs`,
  `os/{windows,macos}.rs`); needs the bar stack integrated first.
- `komorebi` (daemon): A. Keep three library crates, rename to `komorebi-windows`, `komorebi-linux`,
  `komorebi-macos` (distinct lib names, `komorebi_windows` etc.), each gated to its OS
  (`#![cfg(target_os = ...)]`, target-gated dependencies, so `--workspace` works everywhere), each
  keeping its own `crate::` paths and its own tests (KDE's 881 run unchanged but for the name). One
  thin `komorebi` binary package: `src/main.rs` is a `cfg_select!` over the three daemons'
  `main` bodies (each daemon's present `main.rs` becomes `pub fn run()` of its library). Option B for
  the daemon would be a rewrite of about 1,700 `crate::` paths (669 + 447 + 587) for no behavioural gain, and it would
  put KDE's tests and the VM suites on a moving target.
- `komorebi-client`: one package with `src/{windows,linux,macos}.rs` (211 / 394 / 225 lines, almost all
  `pub use komorebi::...`), each starting with `use komorebi_windows as komorebi;` and so on.

## Migration order

Every revision keeps the three existing trees building with their present gates. "Gate" says what
must pass before it counts as landed: `C` compile (fast-check, the three checks), `S` the CLI oracle
(`S/step4/oracle-check.sh`, byte-identical for all three platforms; it reads `komorebic/src/main.rs`
of each tree, so it needs its paths updated in the revision that moves a file), `N` nix evaluation
(`nix flake show`), `NB` a nix build of the checks in an idle window, `E` Windows e2e, `V` KDE VM `fast`.

0. Prerequisites, no moves (any time, small): explicit `#[command(name = ...)]` on the three CLIs;
   `build.rs` of `komorebic` stops fetching `applications.json` over the network (the file is checked
   in; a build script that needs `reqwest`, and so openssl, on Linux is what stopped the unified
   package building natively); `pub fn main` and `crate::` -> `super::` in the `komorebic` files.
   Gates C, S. Not behaviour.
1. Package renames inside the present workspaces (KDE: `komorebi-linux` and so on; macOS the same;
   Windows daemon `komorebi-windows` last), with `[lib] name` and `[[bin]] name` unchanged. Gates C,
   N; the Windows daemon rename is `E` (the executable's inputs change).
2. Workspace merge: delete `komorebi-for-kde/Cargo.toml` and `komorebi-for-mac/Cargo.toml`, add their
   crates to the root members, write out the inherited dependencies, one lock (+41 packages), add
   `.cargo/config.toml` with `[env] COMMIT_HASH` (KDE's file moves from `komorebi-for-kde/.cargo`;
   cargo reads it from the working directory upward only). Same revision gates every foreign crate
   (`#![cfg(target_os)]`, target-gated dependencies) so `--workspace` works on all targets. Gates C on
   all three, plus KDE tests unchanged. Cargo-wise this is one revision (the spike's script); it must
   not be split without breaking `--locked`.
3. Flakes, same integration as 2 (nix cannot be half-merged): the root flake builds all three; the two
   nested flakes already take the whole repository as their source (`lib.mkOutputs`), so they become
   thin: `workspaceArgs` change from a subdirectory workspace to `-p <package>`. The KDE source filter
   must list the new member directories. Gates N, then `NB` (deps, clippy, nextest of each system),
   then `V`.
4. `komorebic` unified by `#[path]` (spike 2, minus the hacks): new `komorebic/src/main.rs`, target
   tables, aliases. The old `komorebic-linux`/`-macos` packages stay until step 5 so the old names
   work. Gates C, S, and `E` for `komorebic.exe` (the Windows main moves to `windows.rs`).
5. `komorebic` real moves, one platform per revision: `komorebi-for-kde/komorebic/src/*` to
   `komorebic/src/linux/`, the macOS one to `komorebic/src/macos/`, tests with them, then delete the old
   packages. Pure moves (jj does not follow moves, so each is a delete/modify conflict if another stack
   touches those files; today none does). Gates C, S, tests.
6. `komorebi-client` unified (modules per platform), same pattern. Gates C, E (Windows client is in the
   closure of `komorebi.exe`).
7. `komorebi-bar` unified after the bar stack integrates: `bar-unification.md` plan; Windows bar
   screenshot gate and the Mac hand check are owed by that stack.
8. Daemon: three libraries gated to their OS and one thin `komorebi` package with the `cfg_select!`
   `main`. KDE `main.rs` and the Windows `main.rs` become `run()` in their libraries. Gates C, `E`, `V`,
   Mac hand check.
9. Cutover: one flake output per binary, old per-platform package names, `justfile`, CI
   (`.github/workflows`), docs paths, release version (0.1.42 / 0.1.0 / 0.1.1 become one).

## Risks

1. One lock for three platforms: a `cargo update` for one platform moves the others; crane's dummy
   source includes every member's manifest and the lock, so any dependency change rebuilds the
   dependency derivation of all three systems (today a KDE-only lock change cannot touch Windows). The
   `COMMIT_HASH`-constant fix (flow finding 1) must be in first or every commit rebuilds ~1,000 crates.
2. `build.rs` fetching over the network is not usable in a unified package (openssl on Linux); see step 0.
3. `--workspace` commands (clippy, fmt, nextest, crane) select every member on every target: every
   foreign crate must compile empty (spike 3 shows it works for the daemon; only one crate was
   converted). Without it, feature unification would also turn `komorebi-layouts/darwin` on for
   Windows in a whole-workspace build.
4. The one-key-one-path rule forces distinct dependency keys and aliases per platform module.
5. `CARGO_BUILD_TARGET` is set by the root `nix develop` shell, so native Linux checks must run outside it
   (as `fast-check` does) or pass `--target x86_64-unknown-linux-gnu`.
6. Command names and `--version` text come from `CARGO_PKG_NAME`/the version crate: pin them or the KDE
   `version` test and the docs oracle change. `shadow-rs` major versions differ (2 on Windows, 1 on
   macOS); the unified `komorebic` build script can only use one.
7. The KDE VM suites rebuild on any change to the KDE closure: the merge is one full `V` run.
8. Tests that use `CARGO_BIN_EXE_*` or relative paths (`tests/schemas.rs` writes beside the manifest)
   must move with their package; not exercised beyond the spike.

## Gates by step, in short

Compile only: 0, 4 (but see `E`), 5, 6 at the Rust level; nix evaluation only: 3 until built. Needs
e2e (Windows guest): anything that changes an input of `komorebi.exe`, `komorebic.exe` or the client
(`tools/e2e-needed` says: 1 for the Windows daemon, 4, 6, 8). Needs the KDE VM: 2, 3, 8. Needs a Mac,
by hand: 4, 7, 8, 9.

Mac hand check (type-check proves none of it): `komorebic --version` and `--help` show the macOS
command set and the name `komorebic`; `komorebic start` finds and starts `komorebi`, `stop` stops it;
`komorebic docgen` unchanged; `komorebi --version`; the daemon starts, tiles and `komorebic state`
answers; `komorebi-bar` opens, `--schema` equals `komorebi-for-mac/schema.bar.json`;
the embedded `applications.json` is the macOS one;
`otool -L` of each binary lists no Windows or Linux-only library.

## Conflicts with the open stacks (sequence in a quiet period)

Paths touched by each bookmark relative to `wip-main`:

| bookmark | touches | moves in this plan that conflict |
|---|---|---|
| wip-u-kfw | `komorebi/src/**`, `crates/komorebi-backend/**` | daemon rename and any move of `komorebi/` (steps 1 Windows, 8) |
| wip-u-kfm | `komorebi-for-mac/komorebi/src/**`, `crates/komorebi-backend/**` | macOS daemon rename and moves (1, 8) |
| wip-u-kfde | `komorebi-for-kde/komorebi/{src,tests}/**` | KDE daemon rename (1) and moves (8); tests are edited |
| wip-u-bar | `Cargo.lock`, `komorebi-for-mac/Cargo.lock`, `komorebi-bar/src/**`, `komorebi-for-mac/komorebi-bar/src/**`, `crates/komorebi-bar-core` | step 2 (locks, manifests) and step 7 |
| wip-u-settings | `Cargo.toml`, `Cargo.lock`, `crates/komorebi-settings` | step 2 (root manifest and lock) |
| wip-u-nix | `flake.nix`, `komorebi-for-kde/flake.nix`, `komorebi-for-kde/nix/**` | step 3 |

Not touched by any open stack, so safe now: `komorebic/**` (all three), the `komorebic` tests, the
`version` crates, the `komorebi-client` crates (steps 0, 4, 5, 6 are free of conflicts except the lock
and root manifest lines of step 2/4). Hard conflicts: every `Cargo.lock` and the root `Cargo.toml`
`members` list (resolve by taking one side and running `cargo metadata`; never hand-merge a lock),
`flake.nix` lines for the extracted binaries. Suggested order: integrate kfw, kfm, kfde, bar, settings,
nix first; then steps 0 and 1; then 2 and 3 as one integration; the rest in any order, daemon (8)
last.

## What landed

Only this file. The spike revisions are on `wip-u-pkg-spike` and can be dropped or cherry-picked.
Step 0 and the renames were not landed because they only pay off with step 2, which edits the locks
and manifests that three open stacks edit.

## State of the migration (bookmark wip-u-pkg-mig, on wip-u-pkg-base)

Done, in this order (each revision keeps the three trees building; gates are in the revision messages):

0. `applications.json` committed (build.rs fetch gone), explicit clap names, the KDE closure list fixed.
1. KDE and macOS packages suffixed (`komorebi-linux`, `komorebic-macos`, ...); Windows daemon `komorebi-windows`.
2. One root workspace, one lock (901 packages), `default-members` is the Windows set.
3. `nix/workspace-src.nix`: each flake builds its platform's source plus stubs of every other member,
   `-p` per platform.
4. `komorebic` is one package with a `cfg_select!` over three CLI modules; the help text of each platform
   is a literal (it used to come from the package description).
5. The Linux and macOS CLI files moved into `komorebic/src/{linux,macos}` (pure moves, one per revision);
   the KDE `komorebic` tests are `komorebic/tests/linux_*.rs`. The old `komorebic-linux` and
   `komorebic-macos` packages are shims that include the moved files; they have no tests of their own.
6. `komorebi-client`: the three socket codes made equal (Connection on all), then one package whose
   `lib.rs` is a `cfg_select!` re-exporting `windows.rs`, `linux.rs` or `macos.rs`; old
   `komorebi-client-linux`/`-macos` are shims; `komorebic` uses the one client.

Measured by evaluation (no build): an edit of a Windows-only crate or of `komorebic/src/windows.rs` or
`komorebi-client/src/windows.rs` changes the Windows derivations and no kfde derivation; an edit of a Linux
file changes kfde and no Windows derivation; an edit of a macOS file changes neither. The cache isolation of
the three trees survived the merge and the unified packages.

Costs and limits found on the way:
- The stubbed source is a derivation that crane reads at evaluation (`readDir` in `mkDummySrc`), so
  evaluating another system's attributes needs a builder for that system (decision D-pkg-1).
  `nix flake show --all-systems` and aarch64-darwin drvPaths do not evaluate on a Linux host.
- cargo deny, cargo fmt and taplo run over the merged workspace: deny is called with
  `--workspace --exclude <the other platforms' packages>` and the tree's own deny.toml, fmt gets `-p`;
  none of them has been built.
- cargo allows one path per dependency key: the daemon crates keep distinct keys and the modules alias them
  (`extern crate komorebi_linux as komorebi`, `use crate::komorebi_linux as komorebi`). A dependency with
  no `package =` is named by its lib name, not by its key: write `package = "<same name>"` when the key
  must be the extern name.
- Building both `komorebic` and `komorebic-linux` in one cargo command writes the same binary name twice.
- Comments and docs that name `komorebic/src/main.rs`, `komorebic/tests/*.rs` or the KDE client paths
  (check.rs, ARCHITECTURE.md, scripts/compare-gen.py, the VM check suite) still carry the old paths; they
  are text in files whose change would rebuild the KDE derivations.

## Step 7 for the bar, ready to execute (not executed)

State: `crates/komorebi-bar-core` holds the shared code (config, widgets, `BarPlatform`, schema views); the
two shells are `komorebi-bar` (Windows) and `komorebi-for-mac/komorebi-bar` (package `komorebi-bar-macos`),
see `bar-unification.md` for what stays per platform. Both use `komorebi-bar-core` (one version) and
differ in `main.rs`, `bar.rs`, `os.rs`, `widgets/{komorebi,network,widget,mod}.rs`; Windows adds
`widgets/{keyboard,media,systray}.rs`. `crate::` occurs 111 times (Windows) and 90 (macOS).

B0. Prerequisite (done, step 6): one `komorebi-client`. In `komorebi-for-mac/komorebi-bar/Cargo.toml` change
    `komorebi-client = { package = "komorebi-client-macos", path = "../komorebi-client", ... }` to
    `komorebi-client = { path = "../../komorebi-client", default-features = false }`; the feature
    `schemars` already forwards to `komorebi-client/default`. Gate: macOS fake-CC check of
    `komorebi-bar-macos`; one revision.

B1. Windows bar becomes a module, in place. Create `komorebi-bar/src/windows/`; `git mv`/`jj` move
    `bar.rs config.rs os.rs widgets/` into it and `main.rs` to `windows/mod.rs` (`mod bar; mod config; mod
    os; mod widgets;` stay as they are, `fn main` becomes `pub fn main`). In the moved files replace
    `crate::` by `crate::windows::` (sed, 111 sites; do not touch `komorebi_bar_core::`). New
    `komorebi-bar/src/main.rs`: `mod windows; fn main() -> color_eyre::eyre::Result<()> { windows::main() }`.
    Pure move plus one mechanical edit, two revisions (move, then the paths). Gate: Windows clippy
    `-D warnings`; the bar screenshot gate (owed since the bar stack) and `e2e-needed` says NO for the bar.

B2. macOS bar the same, in its own package: `komorebi-for-mac/komorebi-bar/src/macos/`,
    `crate::` -> `crate::macos::` (90 sites), `main.rs` = `mod macos; fn main() { macos::main() }`. Gate:
    macOS fake-CC check; the Mac hand check.

B3. Merge into one package (one revision, with its flake edit):
    - Move `komorebi-for-mac/komorebi-bar/src/macos` to `komorebi-bar/src/macos`.
    - `komorebi-bar/src/main.rs`: `cfg_select! { target_os = "windows" => { mod windows; use windows as
      platform; } target_os = "macos" => { mod macos; use macos as platform; } }` and `fn main() { platform::main() }`.
    - `komorebi-bar/Cargo.toml`: common `[dependencies]` = what both manifests share (`komorebi-bar-core`,
      `komorebi-client`, `komorebi-themes`, `chrono`, `chrono-tz`, `clap`, `color-eyre`, `crossbeam-channel`,
      `dirs`, `dunce`, `eframe`, `egui-phosphor`, `font-loader`, `hotwatch`, `image`, `lazy_static`, `num*`,
      `parking_lot`, `random_word`, `reqwest`, `schemars`, `serde`, `serde_json`, `sysinfo`, `tracing*`);
      `[target.'cfg(windows)'.dependencies]`: `egui_extras`, `netdev = "0.46"`, `regex`, `systray-util`,
      `tokio`, `which`, `windows`, `windows-core`, both `windows-icons` revisions;
      `[target.'cfg(target_os = "macos")'.dependencies]`: `netdev = "0.40"`, `icns`, `objc2`,
      `objc2-app-kit` (the feature list of the macOS manifest), `raw-window-handle`. The dependency versions
      of the two manifests differ only for `netdev` (0.46 / 0.40), which cargo accepts per target.
    - Old package `komorebi-bar-macos` stays: `komorebi-for-mac/komorebi-bar/src/main.rs` is
      `#[path = "../../../komorebi-bar/src/macos/mod.rs"] mod macos; fn main() { macos::main() }`
      (the moved code names `crate::macos::`, which this root provides).
    - Flakes: the macOS flake `-p komorebi-bar` instead of `-p komorebi-bar-macos`, its source filter takes
      `komorebi-bar/{Cargo.toml,src/main.rs,src/macos/**}` and `isReal` `komorebi-bar`; the root filter
      leaves `komorebi-bar/src/macos` out and the KDE flake needs nothing (it never had a bar; the stub
      is the member's manifest). The `fmt`/`deny` lists drop `komorebi-bar` from the macOS exclusions.
    - Schemas: `schema.bar.json` and `komorebi-for-mac/schema.bar.json` stay; the oracle test in
      `komorebi-bar-core` reads both. `cargo run -p komorebi-bar -- --schema` prints the view of the target OS.
    Gates: Windows clippy, macOS fake-CC check of `komorebi-bar` and `komorebi-bar-macos`, the schema oracle
    (`cargo test -p komorebi-bar-core`), nix evaluation, the isolation test of the evaluation (an edit of
    `komorebi-bar/src/windows/` must not change a macOS derivation and the reverse). Owed: the Windows bar
    screenshot, the Mac hand check.

B4. Converge the remaining differences one revision each, each BEHAVIOUR on one platform and needing its
    screenshot or Mac check: `widgets/komorebi.rs` frame margins, `network.rs`, then the platform seams in
    `bar.rs`/`main.rs` behind `BarPlatform` hooks (list in `bar-unification.md`).

## Owed from the coordinator (heavy gates, idle host; everything below was evaluated, none built)

At bookmark `wip-u-pkg-mig`:

```
tools/gate-lock run --class heavy --label "pkg checks" --est 25 -- nix build -L --no-link \
  .#checks.x86_64-linux.komorebi-workspace-clippy .#checks.x86_64-linux.komorebi-workspace-deny \
  .#checks.x86_64-linux.komorebi-workspace-fmt .#checks.x86_64-linux.komorebi-workspace-toml-fmt \
  .#checks.x86_64-linux.komorebi-workspace-nextest
tools/gate-lock run --class heavy --label "pkg kfde checks" --est 40 -- nix build -L --no-link \
  .#checks.x86_64-linux.kfde-komorebi-workspace-{clippy,doc,deny,fmt,toml-fmt,nextest} \
  .#checks.x86_64-linux.kfde-schema-fresh .#packages.x86_64-linux.kfde-komorebic .#packages.x86_64-linux.kfde-komorebi
nix build -L .#packages.x86_64-linux.komorebic .#packages.x86_64-linux.komorebi
```

then the KDE VM `fast` suites (the KDE package derivations changed), and the Windows e2e gate
(`tools/e2e-needed` says YES: `komorebi-windows`, `komorebic` and `komorebi-client` changed); on a Mac or
a darwin builder `nix build -L .#packages.aarch64-darwin.kfm-komorebic .#packages.aarch64-darwin.kfm-komorebi
.#packages.aarch64-darwin.kfm-komorebi-bar` and the `kfm-` checks. By hand on a Mac: `komorebic --help`,
`--version`, `quickstart`, `start`, `stop`, `state`, `docgen`; `komorebi-bar` still builds from the old
package name. If deny, fmt or taplo fail in the merged workspace, the fix is in the `cargoDenyExtraArgs`,
`cargoExtraArgs` of the three flakes, not in the code.
