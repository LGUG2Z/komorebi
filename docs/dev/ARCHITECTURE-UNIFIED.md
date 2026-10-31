# komorebi after the consolidation: a map of the repository

Status (wip-cand4): current map. Windows, KDE Plasma and macOS build from one workspace and one
`komorebi`, one `komorebic`, one `komorebi-client` and one `komorebi-bar` package, the platform chosen by
`cfg(target_os)`. The cutover (shim packages, one flake, final directories, one version number) is not done;
section 6 lists what is still old. macOS has only been type-checked on Linux and compiled/unit-tested on a
Mac Mini; no macOS daemon behaviour has been observed.

How to build and test: [build-and-test.md](build-and-test.md). Why things are shaped as they are:
[seam.md](seam.md) (the `Backend` trait), [core-model.md](core-model.md) (the shared model),
[settings.md](settings.md), [unified-binaries.md](unified-binaries.md), [bar-unification.md](bar-unification.md),
[backend-switch.md](backend-switch.md). What a person with a Mac has to check: [macos-handcheck.md](macos-handcheck.md).

## 1. Three platform trees, one workspace

The root `Cargo.toml` is the only workspace. It has members in three places:

| Tree | What is in it | Platform |
|---|---|---|
| the repository root (`komorebi/`, `komorebi-bar/`, `komorebic/`, ...) | the Windows daemon library, the unified packages, the Windows-only tools (`komorebi-gui`, `komorebi-shortcuts`, `komorebic-no-console`), `komorebi-layouts`, `komorebi-scrolling`, `komorebi-themes`, `e2e/`, `wix/` | Windows (the original) |
| `komorebi-for-kde/` | the KDE daemon library `komorebi-linux` (`komorebi/`), `komorebi-hotkeys/`, the `version-linux` crate, the NixOS module, the VM test suites (`nix/tests/vm`), KDE docs (`docs/dev`) | Linux, KDE Plasma on KWin (Wayland) |
| `komorebi-for-mac/` | the macOS daemon library `komorebi-macos` (`komorebi/`), the `version-macos` crate, the nix darwin module, macOS docs | macOS |

`komorebi-for-kde/` and `komorebi-for-mac/` still have their own flakes, `justfile`s and docs; the root flake
imports them as `kfde-*` and `kfm-*` (section 5). They have no `Cargo.toml` workspace of their own any more:
`cargo` run inside them uses the root one.

## 2. The packages (what a user runs)

| Package (binary) | Directory | How the platform is chosen |
|---|---|---|
| `komorebi` (`komorebi`) | `komorebi-daemon/` | `src/main.rs` is a `cfg_select!` that calls `daemon::run()` of the library of the target OS; the libraries are target-gated dependencies in `Cargo.toml` |
| `komorebic` (`komorebic`) | `komorebic/` | `src/windows.rs`, `src/linux/`, `src/macos/`, one `cfg` per target; the arguments are in `crates/komorebic-core` |
| `komorebi-client` (library) | `komorebi-client/` | `src/windows.rs`, `src/linux.rs`, `src/macos.rs` re-export the daemon library of the target OS behind one `Connection` |
| `komorebi-bar` (`komorebi-bar`) | `komorebi-bar/` | `src/windows/` and `src/macos/` modules; shared code in `crates/komorebi-bar-core`. **KDE has no bar** |

The daemon libraries are `komorebi-windows` (`komorebi/`), `komorebi-linux` (`komorebi-for-kde/komorebi/`) and
`komorebi-macos` (`komorebi-for-mac/komorebi/`). They have no binary: each has a `daemon.rs` with
`run()`, the old `main`. `-p komorebi-windows` etc. still build, as libraries.

Shim packages exist until the cutover (each is a `#[path]` include of the unified module, same binary
name): `komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`,
`komorebi-bar-macos`. Never build `komorebic` together with `komorebic-linux` or `komorebic-macos`: the
binary name is the same.

## 3. The shared crates (`crates/`)

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

Also shared by path from the Windows tree: `komorebi-layouts`, `komorebi-scrolling`, `komorebi-themes`.

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
  platform-only; most are common). `komorebic` pages in `docs/cli` of each tree are the oracle.
- macOS: everything except compile and unit tests is unverified; [macos-handcheck.md](macos-handcheck.md).
- KDE: no bar; hotkeys through `komorebi-hotkeys` and exported KDE shortcuts; the module is NixOS.
- Windows: the e2e under QEMU (`e2e/`), `komorebi-gui`, `komorebi-shortcuts`, `komorebic-no-console`, wix.

## 5. The oracles (what stops a refactor from changing behaviour silently)

| Oracle | Where | Detects | Run by |
|---|---|---|---|
| Wire goldens | `crates/komorebi-protocol/tests/golden*` | a change of what each daemon sends or accepts on the socket | `cargo test -p komorebi-protocol` |
| Config schemas | `schema.json`, `schema.asc.json` of the three trees; `crates/komorebi-config/tests/schemas.rs`; KDE `komorebi/tests/schemas.rs` | a changed key of `komorebi.json` | `cargo test -p komorebi-config` (KDE: `-p komorebi-linux --test schemas -- --test-threads=1`) |
| Bar schema views | `schema.bar.json` (Windows), `komorebi-for-mac/schema.bar.json`; `crates/komorebi-bar-core/tests/schema_oracle.rs` pins size and hash; `komorebi-bar --schema` prints the view of the target OS | a changed bar configuration | `cargo test -p komorebi-bar-core`; Windows e2e scenario `bar` compares `--schema` byte for byte |
| CLI docs | `docs/cli` of each tree; `crates/komorebic-core/tests/docs_cli.rs` and `platforms.rs` | a changed subcommand or help text | `cargo test -p komorebic-core`; `komorebic docgen --output DIR` regenerates |
| Schema docs completeness | `check_schema_docs.py` (root) | schema entries with no description | by hand |
| Drift of derivations | `nix eval .#packages.x86_64-linux.default.drvPath` and the check derivations | an unintended change of what nix builds | by hand (see build-and-test.md) |
| Backend contract | `komorebi-backend/src/contract.rs` | a backend that breaks a rule of the trait | `cargo test -p komorebi-backend` and each daemon's tests |
| Runtime | Windows e2e (QEMU), KDE VM suites | real behaviour | tier 3 below |

Windows `docs/cli` was found stale against the binary in the step-4 measurement (see the progress notes,
decision D20); regenerate before treating it as an oracle.

## 6. What is still the old layout (cutover list)

Shim packages and their directories under `komorebi-for-kde/` and `komorebi-for-mac/`; `version-linux` and
`version-macos`; three flakes (the root one imports the other two as `kfde-*` and `kfm-*`); three `deny.toml`;
three version numbers (Windows libraries and packages 0.1.42, KDE 0.1.0, macOS library 0.1.1 and its `version` crate 0.1.0). Some docs still describe the old layout; those that were
found wrong have been corrected, [COMPARE.md](../../komorebi-for-kde/docs/dev/COMPARE.md) is marked stale.
`komorebi-for-kde/justfile` still has recipes written for a KDE-only workspace (`just check` runs
`cargo test --workspace`, which on Linux would build the Windows crates): use the commands in
build-and-test.md.

## 7. Feedback loops, from cheap to expensive

| Tier | What | Time | How |
|---|---|---|---|
| 0 | docs and links | none | nothing builds; a docs-only commit leaves every nix check a cache hit, except text under `komorebi-for-kde/` and `komorebi-for-mac/`, which are inputs of their derivations |
| 1 | native Linux: `cargo check/test` of the shared crates and the KDE daemon | seconds to minutes | build-and-test.md, section 2 |
| 2 | cross-compile and nix checks: Windows clippy, macOS type-check with a fake C compiler, KDE non-VM checks, schema and CLI oracles | minutes | build-and-test.md, sections 1 and 3 |
| 3 | e2e: Windows QEMU scenarios, KDE VM suites (`fast`, `slow`) | tens of minutes, batched | build-and-test.md, sections 4 and 5 |
| 4 | a real Mac | by hand | macos-handcheck.md |

### The helper tools (outside the repository)

The agents that did the consolidation used scripts that live outside the repository, in
`/home/jado/komomono-work/tools` on the build host. They are not part of the repository. Proposal, not
done: commit the ones that encode project knowledge under `scripts/dev/`, and leave the host-specific ones out.

| Tool | What it does | Worth committing? |
|---|---|---|
| `fast-check` (+ its README) | runs only the checks that the touched paths need (tier 1 and 2), with persistent target dirs | yes: it is the quickest way to get the right gate for a change; needs its scratch paths made configurable (`FAST_CHECK_SCRATCH`, already an env var) |
| `mac-check.sh` | macOS type-check through a fake C compiler | yes, merged into `fast-check` (it has hard-coded paths to a workspace) |
| `kde-closure-check` | fails when `komorebi-for-kde/nix/root-crates.json` and the KDE cargo closure differ | yes: it protects the KDE source filter; run after any dependency change in the KDE tree or the crates it uses |
| `lite-env` | env vars for builds without debug info | optional (one `.env` file) |
| `gate-lock` | host-wide FIFO lock so heavy gates and timing-sensitive e2e do not overlap | host policy; commit only if more than one person shares a build host |
| `e2e-needed` | says whether a diff touches anything the Windows e2e binaries are built from (decides whether the e2e gate is owed) | yes, next to `fast-check` |
| `host-sample`, `jitter-probe`, `trim-target`, `mini` | measurement and housekeeping for this host (`mini` is an SSH wrapper for one Mac Mini) | no |
