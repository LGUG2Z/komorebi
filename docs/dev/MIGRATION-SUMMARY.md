# The consolidation of komorebi, komorebi-for-kde and komorebi-for-mac: final-state summary

Status (written at wip-cand8, b6f4c2ae, 2026-10-31): a summary for a maintainer who was not there.
Gates for candidate 8 were still running when this was written; the results stated below are the ones
that had finished, with the candidate they ran on. Companion documents: [ARCHITECTURE-UNIFIED.md](ARCHITECTURE-UNIFIED.md)
(the map), [build-and-test.md](build-and-test.md) (commands, flakes), [macos-hand-check-plan.md](macos-hand-check-plan.md)
(what the owner must run on a Mac), [known-issues.md](known-issues.md) (open decisions).

## 1. What the repository is now

Three programs (Windows, KDE Plasma, macOS) were three repositories with copies of the same ideas. They are
one repository with one Cargo workspace (one `Cargo.lock`), shared crates under `crates/`, and one package
per user-facing program, the platform chosen by `cfg(target_os)`.

| Package (binary) | Directory | How the platform is chosen |
|---|---|---|
| `komorebi` | `komorebi-daemon/` | `main.rs` is a `cfg_select!` calling `daemon::run()` of `komorebi-windows`, `komorebi-linux` or `komorebi-macos`, which are target-gated dependencies |
| `komorebic` | `komorebic/` | `src/windows.rs`, `src/linux/`, `src/macos/`; the arguments are in `crates/komorebic-core` |
| `komorebi-client` | `komorebi-client/` | `src/{windows,linux,macos}.rs` behind one `Connection` |
| `komorebi-bar` | `komorebi-bar/` | `src/windows/`, `src/macos/`; shared part in `crates/komorebi-bar-core`. KDE has no bar |

Three platform trees remain, because the platform layers are different code: the repository root (Windows:
`komorebi/` is the library `komorebi-windows`), `komorebi-for-kde/` (`komorebi-linux`, hotkeys, NixOS module,
the VM suites) and `komorebi-for-mac/` (`komorebi-macos`, nix-darwin module). The daemon libraries have no
binary; the `komorebi` package is the binary. The crate map and the tree layout are in
[ARCHITECTURE-UNIFIED.md](ARCHITECTURE-UNIFIED.md); how to build each is in [build-and-test.md](build-and-test.md).

## 2. What was shared, and how

| Crate | What it holds | How it was moved |
|---|---|---|
| `komorebi-collections` | `Ring`, `LockableSequence` | the KDE copy was canonical; Windows and macOS switched to it |
| `komorebi-protocol` | `SocketMessage`, payloads, state types | one enum for all platforms (guard arms for platform-only messages); the wire format is pinned by per-platform goldens generated from the three daemons before the move |
| `komorebi-config` | `StaticConfig` and the `komorebi.json` types, schema generation, and the **one window-rule matcher** (`komorebi_config::rules`: `should_act`, `should_act_individual`; see [rule-matching.md](rule-matching.md)) | one union type with per-platform schema views; the matcher replaced three copies, each daemon keeps a thin wrapper with its own signature |
| `komorebi-transport` | the command listener, pause filter, subscriber registry, `EventChannel` | the daemons take the window manager lock; the crate does not name a mutex |
| `komorebi-backend` | the `Backend` trait (what a daemon asks of its platform), plain-data infos, `FakeBackend`, contract tests, the id `Registry` | derived from the evidence in [seam.md](seam.md); Windows goes through it for the clusters in [backend-switch.md](backend-switch.md); KDE runs on it; macOS has an adapter the daemon does not use yet |
| `komorebi-core` | the model, generic over the platform window: `Container<W>`, `Workspace<P>` (with `update_shared`, a skeleton with hooks), `Monitor<P>`, `WorkspaceGlobals`, and `command` (the shared commands over a `CommandHost`: workspace navigation, moves across workspaces and monitors, lock and unlock, focus-workspace with the pointer preamble) | converge in place, then pure move, then converge per daemon; hooks (`WorkspacePlatform`, `MonitorPlatform`, `HideSpot`) carry what differs; details in [core-model.md](core-model.md) |
| `komorebi-settings` | the run-time settings as one immutable snapshot replaced as a whole, instead of process-wide statics (paddings, aspect ratio, layout defaults, display preferences, animation, transparency, window-handling behaviour) | see [settings.md](settings.md) |
| `komorebi-bar-core` | bar configuration, widgets, platform seam, schema views | [bar-unification.md](bar-unification.md) |
| `komorebic-core` | the command line of every platform (clap definitions, docgen), platform enums | the generated `docs/cli` pages were the oracle |

The state JSON each daemon prints is unchanged by the shared `Monitor<P>`/`Workspace<P>` (platform extras are
flattened in, pinned by tests). Native tests of the shared code run on Linux and protect all three daemons
([e2e-to-unit.md](e2e-to-unit.md) says which e2e checks became native).

## 3. What intentionally stays per platform, and why

- **The window manager, the command router and event processing.** About 26-37% of the text is shared and the
  rest is real behaviour (Win32 cloaking, rule engines with different lists, KWin desktops, AppKit
  accessibility). `WindowManager<P>` is not built: [core-model.md](core-model.md) sections 17, 18, 20 and 24 list the
  blockers per platform; section 21 documents the commands that were shared; section 23 is a design note for a
  common event vocabulary ("thin events"), not implemented.
- **macOS `Workspace::update`.** It differs in a scrolling early return and a wider SkyLight disable/re-enable
  batch; adopting `update_shared` would change scrolling on macOS and cannot be verified without a Mac
  (section 22; decision D15 below).
- **Backends and subsystems**: borders, animations, DWM ghosts, KWin bridge and effects, AppKit observers,
  wallpapers, hotkeys. The trait is the seam; the rules are in [seam.md](seam.md).
- **The bar's rendering code** (frame margins, `network.rs`, the window seams in `bar.rs`/`main.rs`): step B4 of
  [bar-unification.md](bar-unification.md) was not done because it cannot be shown mechanical without a screen.
- **Command line**: each platform keeps its own commands and help text; most are common.
- **Packaging**: [unified-binaries.md](unified-binaries.md): one binary, a library per OS.

## 4. How it was verified

### Oracles (checked in files, compared byte for byte)

Wire goldens (`komorebi-protocol`), the three `schema.json` and `schema.asc.json`, the two `schema.bar.json`
(size and hash pinned in `komorebi-bar-core`), the `docs/cli` pages of each tree, JSON pins of `Container`,
`Monitor` and the state of the three daemons from real fixtures, and derivation drift (`nix eval ...drvPath`).
Details in [ARCHITECTURE-UNIFIED.md](ARCHITECTURE-UNIFIED.md) section 5.

### Gates, by candidate

Each candidate was a merge of the finished stacks, gated once. Results as recorded (host load and flakes noted
in [build-and-test.md](build-and-test.md) section 6):

| Candidate | Windows e2e (QEMU guest) | KDE VM | macOS (a Mac Mini: compile, unit tests, nix) | nix pre-landing |
|---|---|---|---|---|
| baseline (before the work) | stale markers (4 checks) fail; `commands` fails about 1 in 10 sweeps | `vm-commands` never green; `vm-core-e2e` once | n/a | n/a |
| 5 (core: Container, Workspace, Monitor, `update_shared`, settings) | 38 scenarios: green apart from the 4 stale markers and flakes (`commands` 265/1 and `config_reload` 7/1 from a lock-hold stall, `resize` 25/1); the `bar` scenario could not open a window (guest has no OpenGL 2.0), schema byte-equal | 16 of 16 fast suites green, `vm-commands` green for the first time | cargo unit tests and clippy green; nix: packages and module checks green after toolchain and source-filter fixes | n/a |
| 7 (shared rule matcher, shared commands, stall fixes) | not run as a whole | not run | cargo green, nix green | Windows clippy green; Windows fmt and deny, and two KDE clippy lints, failed and were fixed in 8; packages built |
| 8 (pointer picks the monitor, bar wgpu fallback, macOS deep dedup, formatting) | in progress at the time of writing: `commands` 266/0 (first fully clean), `config_reload` 8/0 | queued | nix all green, cargo 665 passed, 0 failed, 1 ignored | in progress |

Stress runs of lock-heavy command sequences compared baseline and candidate 5: non-answers of `komorebic state`
fell from 80-83% to 14-15%, and "border manager stuck" warnings from 21-31 to 3-5 (the stall fixes work; the
stall itself still occurs now and then).

### Flake registry (summary; full table in build-and-test.md)

Known: Windows `commands` stall (about 1 in 10-12 sweeps, cause not found), `lifecycle_hidden`, `resize`, four
stale known-failure markers (`maximizing.user`, `placement` integer case, `unknown_monitor` x2, same on baseline),
a runner quirk for `focus_follows_mouse`. Candidates: daemon start failing with `AllowSetForegroundWindow` at high
load, `hotplug` with the recorder. KDE: `vm-commands` and `vm-core-e2e` were load-dependent on the old baseline.

## 5. What is NOT verified

- **macOS has never run as a daemon in these builds.** Only compile, clippy, unit tests and nix builds ran on a
  Mac. Every behaviour change that touches macOS (hiding, the registry, the shared Container API, settings,
  pointer-picks-monitor, bar clicks) is listed in [macos-hand-check-plan.md](macos-hand-check-plan.md).
- **The bar at run time.** The Windows `bar` scenario passes schema and config checks; the guest has only
  software rendering, so window checks depend on the new wgpu fallback (WARP), which had not run in a guest when
  this was written. The macOS bar never rendered under these changes.
- **The KDE `slow` VM tier** (18 suites: keys, rdp, drag and live resize, lock-sleep, wallpaper, look, ...) was not
  run; only the `fast` tier (16 suites) was, on candidate 5. Candidate 8 had not been run on the VM yet.
- Windows: no scenario covers transparency or animation; only `config_reload` touches the settings path.
- Behaviour changes marked BEHAVIOUR in revision bodies are landed only after the e2e or VM gate named in the body.

## 6. Known issues

From [known-issues.md](known-issues.md): (1) KDE skipped the pointer-monitor preamble for three workspace
commands, now resolved for all platforms by owner decision; (2) `Monitor::focus_workspace` overwrites a workspace
name; (3) `Workspace::remove_container` focuses the container next to the focused one rather than the removed
one; (4) a subscriber that stops reading can stall the daemon on macOS (a 1 s write timeout was tried and reverted
because it can truncate a notification). Items 2-4 are inherited behaviour, pinned by tests, left for a decision.
Other open items: the Windows lock-hold stall; `komorebi-for-kde/justfile` recipes assume a KDE-only workspace;
`docs/dev/COMPARE.md` and `scripts/compare-gen.py` are stale.

## 7. Cutover list (not done)

What must be deleted or merged to finish; nothing below was changed.

| Item | State | Action |
|---|---|---|
| shim packages | `komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`, `komorebi-bar-macos` are `#[path]` includes of the unified modules | delete them and their directories under `komorebi-for-kde/` and `komorebi-for-mac/`; update the flakes and justfiles that name them |
| version crates | `version-linux`, `version-macos` | one `version` crate; one workspace version (Windows 0.1.42, KDE 0.1.0, macOS 0.1.1 today) |
| flakes | three flakes; the root one imports the other two as `kfde-*` and `kfm-*` | one flake with the platform trees moved to final directories; KDE's VM suites and module, macOS's darwin module stay as outputs |
| `deny.toml`, `rustfmt.toml`, toolchain files | one per tree | one of each |
| workflows | root: `windows.yaml`, `feature-check.yaml`; KDE: `ci.yaml` (nix checks, VM tiers, canary, release), `update-flake-lock.yaml`, `rerun-no-kvm.yaml`; macOS: `nix.yaml`, `release.yaml` (Homebrew formula) | a proposal, not decided: one CI that builds the three packages, runs `crates/` natively, the nix checks, the Windows e2e on a host with KVM, and the KDE VM tiers as today; one release workflow producing the Windows, Linux and macOS artifacts with one tag and one version |
| docs | per-tree `docs/`, CONTRIBUTING, READMEs | one docs tree; `docs/cli` per platform stays until commands are merged; retire COMPARE.md |
| justfiles | three | one |
| tools | `tools/` is committed (fast-check, format-all, kde-closure-check, e2e-needed, gate-lock, ...); see `tools/README.md` | keep; delete the per-tree copies of what they replace |

## 8. How to continue

1. **Get the macOS pass done first.** [macos-hand-check-plan.md](macos-hand-check-plan.md). Nothing deeper should land
   on macOS before it.
2. **`WindowManager<P>`.** Blockers per platform: [core-model.md](core-model.md) sections 20 (KDE, macOS) and 24
   (Windows). The unmanaged-window gate is not behaviour-neutral to move into the router (a `gate_message` hook is
   proposed in section 22.1). Rule: gate every step with the e2e/VM suites, one daemon at a time.
3. **Thin events.** Design note in section 23 (`CoreEvent<W>`, shared handlers for gone, appeared and minimized);
   risks: side-effect order per arm, macOS focus change carries no window.
4. **Commands.** `komorebi_core::command` is partial: sixteen arms and focus-workspace are shared; the remaining
   arms are per platform (section 21 lists what stays and why). **The rule matcher is done** (one implementation).
5. **The deeper-dedup boundary.** Share a function only when a native test can pin it; where the text differs for
   a real platform reason, put the difference behind a hook (as `HideSpot` and `begin_pass` do) rather than
   converging behaviour you cannot verify.
6. **Decide items 2-4 of the known issues**, then move them into the shared code with the e2e gate.
7. **Complete the cutover** (section 7), then regenerate the Windows `docs/cli` and the schema docs.

## 9. The decisions that shaped the design

Rewritten from the decisions log. "Owner" marks a decision the owner made; the others were made to unblock and are
reversible as stated.

| # | Decision | Why | Reverse |
|---|---|---|---|
| 1 | One workspace, one `Cargo.lock`; flakes re-export the nested ones and the root builds them from the whole repo | a spike showed 859 to 900 packages with no conflicts; the root source can then be shared | keep three workspaces |
| 2 | macOS checks on Linux use a fake C compiler and only type-check | nothing else runs without a Mac | none |
| 3 | The wire oracle is a schema-driven golden generator run on the three daemons before the move | the move must not change what is sent | none |
| 4 | One union `StaticConfig` with per-platform schema views; per-platform defaults documented in a table instead of converging every doc | users' files must keep loading; schemas are checked in | one type per platform |
| 5 | `komorebic-core` generic over the platform, no `cfg` in the definitions; the CLI docs of each tree are the oracle | the pages are what users read | per-platform clap structs |
| 6 | Shared transport: the daemon locks the window manager, the crate never names a mutex | different locks per daemon | none |
| 7 | The bar is `komorebi-bar-core` plus a platform trait, one config enum, schema = per-platform views of one union; macOS accepts and skips Windows-only keys with a warning | one config file works everywhere | separate bars |
| 8 | `Backend` is derived from evidence (seam.md), not written from memory of one platform | a trait written from one platform bakes in its assumptions | none |
| 9 | Settings handle is `RwLock<Arc<Settings<P>>>` with a writer mutex; the platform part is a type parameter | no new dependency; one consistent snapshot per use | use `ArcSwap` |
| 10 | Unified layout: one package each for `komorebic`, `komorebi-bar`, `komorebi-client`; the daemon as three OS-gated libraries behind one thin binary | rewriting about 1700 `crate::` paths and moving KDE tests was the alternative | merge the libraries later |
| 11 | Shared `Monitor<P>` keeps each daemon's state JSON (flattened platform extras); the runtime `required` order in generated schemas may change | state JSON is pinned by fixtures and user tooling | one superset JSON |
| 12 | `Workspace`/`Monitor` move by classifying each difference (cosmetic, same behaviour, real, missing), converging, then moving | a 25-member trait would hide real differences | per-platform copies |
| 13 | `Workspace<P>` hook signatures: fallible `begin_update`, `end_update` returns a pass, `place_window` returns a result; the Workspace move is atomic | review of the first design against KDE and Windows | finer hooks |
| 14 | Windows `komorebic visible-windows` lists unreadable windows with empty strings | a shared `visible_window_details` needs infallible details | restore the skip |
| 15 | `Layout::Custom` stays behind a Windows hook in the shared Workspace | unreachable on KDE; no KDE change | move the arms later |
| 16 | The KDE `vm-commands` handover failure is a test-precondition bug: fix the scenario, keep the check strict | an empty workspace cannot recalculate its layout; a check that cannot pass tests nothing | weaker check (rejected) |
| 17 | Freeze new behavioural stacks and stop the model move at the `WindowManager` boundary until gates catch up | shared text is 26-37%; gates were the critical path | resume sharing after gates |
| 18 | Revert the 1 s subscriber write timeout; fix the stale bar-running flag, the hwnd writer, dead-window bookkeeping, and add `komorebi-core` to default members | the timeout can truncate a notification and does not bound the stall | none (design needed: a bounded queue per subscriber) |
| 19 | Owner: KDE and macOS get the Windows pointer-picks-the-monitor behaviour for the workspace commands | consistency across platforms | per-platform preamble |
| 20 | Owner: explore deeper deduplication where it improves cross-platform testing | tests protect all three | stop at the boundary |
| 21 | Owner: software OpenGL fallback for the Windows bar (eframe `wgpu`, glow first, retry on failure) | the e2e guest has no OpenGL 2.0 | remove the feature |
| 22 | macOS keeps its own `Workspace::update` | adopting `update_shared` changes scrolling and cannot be verified without a Mac | adopt after the Mac pass |
| 23 | Formatting of real sources is applied last, with `tools/format-all` | formatting early conflicts with every open stack | none |
| 24 | The rule-list lock order is documented at the statics of each daemon (KDE, macOS, Windows) | two orders deadlocked on reload | none |
| 25 | Constant commit stamp (`nix-check`, `e2e`) in checks and e2e builds; only `packages.default` carries the real commit | a docs-only commit must be a cache hit | none |
| 26 | Gates are batched into candidates; a rebase invalidates a claim, so the gate is re-run at the tip | one revision compiled only because of a later fix | none |
