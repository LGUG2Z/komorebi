# komorebi after the consolidation: a map of the repository

Status (the cutover stack, `wip-cutover`): current map. Windows, KDE Plasma and macOS build from one
workspace (version 0.2.0) and one `komorebi`, one `komorebic`, one `komorebi-client` and one `komorebi-bar`
package, the platform chosen by `cfg(target_os)`. The shim packages are gone, the directories are the final
ones and there is one `komorebi-version` crate; the one flake is being written by another agent
(`flake-one`) and CI and releases are not done: section 6 says what is done and what is open. macOS has
only been type-checked on Linux and compiled/unit-tested on a Mac Mini; no macOS daemon behaviour has been
observed. Documents below that give a measurement or a decision keep the paths and numbers of their time
(`komorebi-for-kde/`, `komorebi-for-mac/`, 0.1.42, `kfde-*`); the table in section 1 says where things are now.

How to build and test: [build-and-test.md](build-and-test.md). Why things are shaped as they are:
[seam.md](seam.md) (the `Backend` trait), [core-model.md](core-model.md) (the shared model),
[settings.md](settings.md), [unified-binaries.md](unified-binaries.md), [bar-unification.md](bar-unification.md),
[backend-switch.md](backend-switch.md). What a person with a Mac has to check: [macos-handcheck.md](macos-handcheck.md).

## 1. Three platforms, one workspace, one rule for the directories

The root `Cargo.toml` is the only workspace. One rule says where a member lives:

- a **shipped binary** has a directory at the top level: `komorebi/`, `komorebic/`, `komorebic-no-console/`,
  `komorebi-bar/`, `komorebi-gui/`, `komorebi-shortcuts/` and `komorebi-recorder/` (with its `wix/` and
  `docs/recorder/`). The directory of the package is its name, so `cargo install --path komorebi` installs
  the `komorebi` binary;
- every **library** and every **development tool** lives under `crates/<package name>/`: the daemon
  libraries of the three platforms, the shared crates, the end-to-end guests (`crates/komorebi-e2e-agent`,
  `crates/komorebi-testwin`), `crates/schemars-nixgen` and `crates/schemars-docgen`.

The platform-specific parts are:

| Platform | Where it is | What is in it |
|---|---|---|
| Windows (the original) | `crates/komorebi-windows/` (the daemon library), `komorebi-bar/`, `komorebic/`, `komorebi-gui/`, `komorebi-shortcuts/`, `komorebic-no-console/`, `crates/komorebi-layouts/`, `crates/komorebi-scrolling/`, `crates/komorebi-themes/`, `crates/komorebi-e2e-agent/`, `crates/komorebi-testwin/`, `e2e/`, `wix/`; docs in `docs/` | the Windows daemon library, the unified packages, the Windows-only tools |
| Linux, KDE Plasma on KWin (Wayland) | `crates/komorebi-linux/` (the daemon library), `crates/komorebi-hotkeys/`, `nix/linux/` (NixOS module, VM test suites in `nix/linux/tests/vm`, KWin effects), `scripts/linux/`, `spikes/`; docs in `docs/linux/` | the KDE daemon library, the key binding crate, the NixOS module and the VM tests |
| macOS | `crates/komorebi-macos/` (the daemon library), `nix/macos/`; docs in `docs/macos/` | the macOS daemon library and the nix-darwin module |

Shared: `crates/` (the libraries; including `crates/komorebi-version`, the version string of the Linux and macOS
binaries), `schemas/{windows,linux,macos}/` (the generated JSON schemas), one `deny.toml`, `rustfmt.toml`,
`rust-toolchain.toml`, `.gitignore`, `LICENSE.md` and `justfile` at the root; `starlight/` (the docs site, an Astro
Starlight project whose reference pages `just docgen` writes with `crates/schemars-docgen`). The old repositories'
workflows, dependabot files, the Homebrew formula template and the macOS docs-site files are kept, inert, in
`docs/dev/old-ci/{linux,macos}/` for the CI and release plan. There is no `komorebi-for-kde/` or
`komorebi-for-mac/` any more except the two flakes that `flake-one` is replacing.

## 2. The packages (what a user runs)

| Package (binary) | Directory | How the platform is chosen |
|---|---|---|
| `komorebi` (`komorebi`) | `komorebi/` | `src/main.rs` is a `cfg_select!` that calls `daemon::run()` of the library of the target OS; the libraries are target-gated dependencies in `Cargo.toml` |
| `komorebic` (`komorebic`) | `komorebic/` | `src/windows.rs`, `src/linux/`, `src/macos/`, one `cfg` per target; the arguments are in `crates/komorebic-core` |
| `komorebi-client` (library) | `crates/komorebi-client/` | `src/windows.rs`, `src/linux.rs`, `src/macos.rs` re-export the daemon library of the target OS behind one `Connection` |
| `komorebi-bar` (`komorebi-bar`) | `komorebi-bar/` | `src/windows/` and `src/macos/` modules; shared code in `crates/komorebi-bar-core`. **KDE has no bar** |

The daemon libraries are `komorebi-windows` (`crates/komorebi-windows/`), `komorebi-linux` (`crates/komorebi-linux/`) and
`komorebi-macos` (`crates/komorebi-macos/`). They have no binary: each has a `daemon.rs` with
`run()`, the old `main`. `-p komorebi-windows` etc. build them, as libraries.

The shim packages (`komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`,
`komorebi-bar-macos`, each a `#[path]` include of the unified module) were deleted in the cutover stack, and
so were `version-linux` and `version-macos` (now `crates/komorebi-version`). To build a platform use the
one package name, for example `-p komorebic -p komorebi-client -p komorebi-bar -p komorebi-macos` on macOS.

## 3. The libraries and tools (`crates/`)

Every library and tool of the workspace is a directory of `crates/`, named as its package; the shared ones are:

| Crate | What it holds | Used by |
|---|---|---|
| `komorebi-protocol` | the wire types: `SocketMessage` and payloads; per-platform goldens in `tests/golden/` | daemons, client, komorebic |
| `komorebi-config` | the `komorebi.json` types (`StaticConfig` and what it is made of) and schema generation | daemons, komorebic |
| `komorebi-collections` | `Ring`, `LockableSequence` | the three daemons |
| `komorebi-transport` | the command transport: pause filter, listener, subscriber registry | the three daemons |
| `komorebi-backend` | the `Backend` trait (what a daemon asks of its platform), `WindowId`, plain-data infos, `FakeBackend`, the contract tests every backend must pass | daemons; Windows goes through it for the clusters in backend-switch.md |
| `komorebi-core` | the shared model, generic over the platform window: `Container<W>`, `Monitor<P>`, `Workspace<P>` with `update_shared`, `WorkspaceGlobals`. `WindowManager<P>` is not shared yet | the three daemons |
| `komorebi-settings` | the settings a daemon reads at run time as one immutable snapshot, instead of process-wide statics | the three daemons |
| `komorebi-bar-core` | the bar's configuration, widgets and platform seam | `komorebi-bar` |
| `komorebic-core` | the command line definitions of every platform (`cli`, `docgen`, `platform::{windows,macos,...}`) | `komorebic` |

Also under `crates/`: `komorebi-layouts`, `komorebi-scrolling`, `komorebi-themes` (Windows-origin, platform-neutral), `komorebi-client`, and the daemon libraries (section 2).

## 4. Where the platforms differ on purpose

These are intentional, not leftovers; before "unifying" one, read the document named.

- Window system and hiding: Win32 (cloaking, own border windows, DWM animations), KWin (a JavaScript
  bridge loaded into KWin, desktops as workspaces, parking desktop), AppKit/Accessibility (own `NSWindow`
  borders). `komorebi-backend` is the seam; the rules and what each platform keeps are in [seam.md](seam.md).
- The model: `Container`, `Monitor`, `Workspace` are shared; `WindowManager`, the command router and event
  processing are still per platform, with the blockers listed in [core-model.md](core-model.md) sections 17
  and 18.
- Settings: [settings.md](settings.md) says which process-wide statics became snapshot fields and which
  remain.
- Packaging: [unified-binaries.md](unified-binaries.md): library per OS, one binary; `cfg(target_os)`, not
  features.
- The bar: [bar-unification.md](bar-unification.md): one package, one module per OS; rendering code differs
  (frame margins, `network.rs`, window seams) and B4 (merging it) was not done.
- Command line: each platform keeps its help text and its own commands (Windows 34, KDE 2, macOS 1 are
  platform-only; most are common). `komorebic` pages in `docs/cli`, `docs/linux/cli` and `docs/macos/cli` are the oracle.
- macOS: everything except compile and unit tests is unverified; [macos-handcheck.md](macos-handcheck.md).
- KDE: no bar; hotkeys through `komorebi-hotkeys` and exported KDE shortcuts; the module is NixOS.
- Windows: the e2e under QEMU (`e2e/`), `komorebi-gui`, `komorebi-shortcuts`, `komorebic-no-console`, wix.

## 5. The oracles (what stops a refactor from changing behaviour silently)

| Oracle | Where | Detects | Run by |
|---|---|---|---|
| Wire goldens | `crates/komorebi-protocol/tests/golden*` | a change of what each daemon sends or accepts on the socket | `cargo test -p komorebi-protocol` |
| Config schemas | `schemas/{windows,linux,macos}/schema.json` and `schema.asc.json`; `crates/komorebi-config/tests/schemas.rs`; KDE `komorebi-linux/tests/schemas.rs` | a changed key of `komorebi.json` | `cargo test -p komorebi-config` (KDE: `-p komorebi-linux --test schemas -- --test-threads=1`) |
| Bar schema views | `schemas/windows/schema.bar.json`, `schemas/macos/schema.bar.json`; `crates/komorebi-bar-core/tests/schema_oracle.rs` pins size and hash; `komorebi-bar --schema` prints the view of the target OS | a changed bar configuration | `cargo test -p komorebi-bar-core`; Windows e2e scenario `bar` compares `--schema` byte for byte |
| CLI docs | `docs/cli`, `docs/linux/cli`, `docs/macos/cli`; `crates/komorebic-core/tests/docs_cli.rs` and `platforms.rs` | a changed subcommand or help text | `cargo test -p komorebic-core`; `komorebic docgen --output DIR` regenerates |
| Schema docs completeness | `check_schema_docs.py` (root; the macOS copy is archived in `docs/dev/old-ci/macos/`) | schema entries with no description | by hand |
| Drift of derivations | `nix eval .#packages.x86_64-linux.windows-default.drvPath` and the check derivations | an unintended change of what nix builds | by hand (see build-and-test.md) |
| Backend contract | `komorebi-backend/src/contract.rs` | a backend that breaks a rule of the trait | `cargo test -p komorebi-backend` and each daemon's tests |
| Runtime | Windows e2e (QEMU), KDE VM suites | real behaviour | tier 3 below |

Windows `docs/cli` was found stale against the binary in the step-4 measurement (see the progress notes,
decision D20); regenerate before treating it as an oracle.

## 6. The cutover: done, and still open

Done in the cutover stack (`jj log -r 'wip-main..wip-cutover'`, one revision per concern):

- one workspace version, 0.2.0, for every package (the tables of older documents that say 0.1.42, 0.1.0
  and 0.1.1 are history); one `komorebi-version` crate instead of `version-linux` and `version-macos`;
- the shim packages `komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`
  and `komorebi-bar-macos` deleted;
- the directories renamed (at that time; since then every library moved under `crates/`, and `komorebi-daemon/` became `komorebi/`, see section 1): `komorebi/` to `komorebi-windows/`, the KDE and macOS libraries to
  `komorebi-linux/` and `komorebi-macos/`, `komorebi-hotkeys/` to the top level; `docs/{linux,macos}`,
  `nix/{linux,macos}`, `scripts/linux`, `spikes/`, `schemas/{windows,linux,macos}`;
- one `deny.toml`, `rustfmt.toml`, `rust-toolchain.toml`, `.gitignore`, `LICENSE.md` and `justfile`;
  `tools/format-all` checks one group;
- the documents, scripts and comments that named the old paths name the new ones (the Windows docs site
  excludes `docs/linux`, `docs/macos` and `docs/dev`).

Still open:

- the one root flake: three flakes become one, written by the agent `flake-one`; until it lands the
  nix commands in [build-and-test.md](build-and-test.md) and the flake attribute names in the scripts and
  runbooks (`.#kfde-...`, `.#kfm-...`, `legacyPackages.*.vm.NAME`) are the old ones;
- CI and release: the workflows of the old repositories are archived, inert, in `docs/dev/old-ci/`; no
  workflow builds the Linux or macOS packages, and there is one release plan to write;
- where the repository and the sites are published: URLs of the old projects
  (`LGUG2Z/komorebi-for-kde`, `LGUG2Z/komorebi-for-mac`, `komorebi-for-mac.lgug2z.com`, the update check of
  the macOS bar, `$schema` URLs of example configs and schemas) are unchanged;
- `docs/linux/dev/COMPARE.md` and `scripts/linux/compare-gen.py` are stale (marked so);
- `docs/linux/dev/ARCHITECTURE.md` and the other KDE documents were written for a KDE-only repository;
  their paths were fixed, their prose about "this repository" was not rewritten.

## 7. Feedback loops, from cheap to expensive

| Tier | What | Time | How |
|---|---|---|---|
| 0 | docs and links | none | nothing builds; a docs-only commit leaves every nix check a cache hit, except text under `nix/linux/` and `nix/macos/` and the sources the derivations use |
| 1 | native Linux: `cargo check/test` of the shared crates and the KDE daemon | seconds to minutes | build-and-test.md, section 2 |
| 2 | cross-compile and nix checks: Windows clippy, macOS type-check with a fake C compiler, KDE non-VM checks, schema and CLI oracles | minutes | build-and-test.md, sections 1 and 3 |
| 3 | e2e: Windows QEMU scenarios, KDE VM suites (`fast`, `slow`) | tens of minutes, batched | build-and-test.md, sections 4 and 5 |
| 4 | a real Mac | by hand | macos-handcheck.md |

### The helper tools

The agents that did the consolidation used these scripts. They were first kept outside the repository (at
the time of the table below, `/home/jado/komomono-work/tools` on the build host); the cutover stack has them
in `tools/` (see `tools/README.md`); `host-sample`, `jitter-probe` and `mini`, which are about one host, were
left out. The table is the original assessment.

| Tool | What it does | Worth committing? |
|---|---|---|
| `fast-check` (+ its README) | runs only the checks that the touched paths need (tier 1 and 2), with persistent target dirs | yes: it is the quickest way to get the right gate for a change; needs its scratch paths made configurable (`FAST_CHECK_SCRATCH`, already an env var) |
| `mac-check.sh` | macOS type-check through a fake C compiler | yes, merged into `fast-check` (it has hard-coded paths to a workspace) |
| `kde-closure-check` | fails when `nix/linux/root-crates.json` and the KDE cargo closure differ | yes: it protects the KDE source filter; run after any dependency change in the KDE tree or the crates it uses |
| `lite-env` | env vars for builds without debug info | optional (one `.env` file) |
| `gate-lock` | host-wide FIFO lock so heavy gates and timing-sensitive e2e do not overlap | host policy; commit only if more than one person shares a build host |
| `e2e-needed` | says whether a diff touches anything the Windows e2e binaries are built from (decides whether the e2e gate is owed) | yes, next to `fast-check` |
| `host-sample`, `jitter-probe`, `trim-target`, `mini` | measurement and housekeeping for this host (`mini` is an SSH wrapper for one Mac Mini) | no |
