# Build and test, per platform

Status: the nix commands name the outputs of the one root flake ([flake.md](flake.md) has the
old-name to new-name table and the layout). Commands marked "ran" were run on the Linux host at the revision
that made the flake; the rest are documented from `flake.nix`, the justfile, the Cargo manifests or the e2e
README. macOS cannot be built or run on Linux; Linux can only type-check it.

The map of the repository is [ARCHITECTURE-UNIFIED.md](ARCHITECTURE-UNIFIED.md). Package names in this
repository (`cargo metadata --no-deps`, ran): `komorebi` (the daemon, `komorebi/`), `komorebic`,
`komorebi-client`, `komorebi-bar`, the libraries `komorebi-windows`, `komorebi-linux`, `komorebi-macos`, the
shims `komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`,
`komorebi-bar-macos`, and `komorebi-backend`, `-bar-core`, `komorebic-core`, `-collections`, `-config`,
`-core`, `-protocol`, `-settings`, `-transport`.

## 0. General rules

- Platform is chosen by `cfg(target_os)`: `cargo build -p komorebi -p komorebic -p komorebi-bar` builds the
  program of the OS you are on. The Windows daemon is only buildable for the Windows target (cross build in
  section 1); on Linux `-p komorebi` is the KDE daemon, on macOS the macOS one.
- Never put `komorebic` and `komorebic-linux`/`komorebic-macos` in one command (same binary name). The shims
  stay until the cutover.
- Without a `.git` (a jj workspace) the build script of the macOS and Linux crates needs `COMMIT_HASH=dev`. Do
  not run a plain `nix flake check` on the root flake (it boots the VM suites); name the check, or run
  `nix flake check --no-build --all-systems` for the evaluation only.
- Text edits under `crates/komorebi-linux/`, `crates/komorebi-hotkeys/` or `crates/komorebi-macos/` change only that platform's nix
  derivations (each platform builds from its own filtered source, `nix/workspace-src.nix`); a docs-only edit
  elsewhere changes none, because the commit stamp is a constant in the checks (`COMMIT_HASH=nix-check`) and
  only the native `default` and `windows-default` packages carry the real commit.
- Heavy builds and e2e should not run at the same time as a timing-sensitive e2e: the Windows `commands`
  scenario is the most sensitive (section 6).

## Developer tools

`tools/` (README: [tools/README.md](../../tools/README.md)) holds the helper scripts, runnable from any jj
workspace of the repository; none of them writes to the source tree.

| Command | Use |
|---|---|
| `tools/fast-check [--list] [--range \| --since REV]` | only the checks the touched paths need; warm in seconds (Windows clippy 8-9 s, KDE 1-3 s, macOS 2-3 s) |
| `tools/e2e-needed [--since REV]` | does this diff change the Windows e2e binaries (`NO` carries a previous passing e2e over) |
| `tools/kde-closure-check` | the Linux source filter list (`nix/linux/root-crates.json`) matches the Linux cargo closure |
| `tools/test-partition check \| packages CLASS \| expr CLASS` | which package is tested where (`tools/test-partition.json`); `just test-partition-check` also runs its self-test |
| `tools/mac-check.sh` | macOS type-check on Linux (fake C compiler `tools/cc-fake.sh`) |
| `tools/trim-target`, `tools/gate-lock`, `tools/lite-env` | disk trimming, a heavy-gate lock, debug-info-off builds |

The persistent target directories are under `$FAST_CHECK_SCRATCH` (default `~/.cache/komorebi-fast-check`).

## 1. Windows (cross build from Linux via nix)

Binaries (the flake cross-compiles to `x86_64-pc-windows-msvc` with an xwin SDK):

```sh
nix build .#windows-komorebi .#windows-komorebic .#windows-komorebi-bar   # result*/bin/*.exe; also windows-komorebic-no-console, -komorebi-gui, -komorebi-shortcuts, -komorebi-testwin
nix build .#windows-default                                  # windows-komorebi-full: everything, with the real commit stamp
```

Checks (crane; the clippy one is the pre-landing gate, about 3.5 min warm deps; the dependency derivation is
shared by all of them):

```sh
nix build -L --no-link .#checks.x86_64-linux.windows-clippy
nix build -L --no-link .#checks.x86_64-linux.windows-nextest-build # cross-compiles the test executables of the windows and os_sensitive packages (tools/test-partition.json), runs none
nix build -L --no-link .#checks.x86_64-linux.windows-fmt .#checks.x86_64-linux.windows-toml-fmt .#checks.x86_64-linux.windows-deny
nix build -L --no-link .#checks.x86_64-linux.windows-e2e-tests # the Python of the e2e (pytest), no guest needed
```

The checks are in sets (docs/dev/flake-checks.md): one set alone builds without the others being evaluated.

```sh
nix build -L --no-link .#checkSets.fmt .#checkSets.deny      # a set (a linkFarm of its members); also linux-lint, linux-test, windows-lint, ...
nix build -L --no-link .#checkSets.cheap                      # what `nix flake check` runs (defaultCheckSets in nix/checks.nix)
nix build -L --no-link .#checkSets.vm-fast                    # the heavy sets: vm-fast, vm-slow, linux-nixos-eval, linux-demo
nix build -L --no-link .#checkMembers.vm-fast.vm-kwin         # one member of any set (what a heavy check is called)
nix flake check --no-build                                    # evaluates the default sets and every other output
```

Quick loop without nix builds (what `tools/fast-check` runs; clippy of the Windows tree from the nix dev shell):

```sh
nix develop --command cargo clippy --locked --all-targets -- -D warnings
```

On a Windows machine itself: `just build` / `just install` (the root `justfile`, PowerShell), or
`cargo build -p komorebi -p komorebic -p komorebi-bar --locked`.

Platform-neutral crates test natively on Linux, with no cross toolchain: `cargo test -p komorebi-protocol
-p komorebi-config -p komorebi-collections -p komorebi-core -p komorebi-backend -p komorebi-settings
-p komorebi-transport -p komorebic-core -p komorebi-bar-core -p komorebi-layouts -p komorebi-scrolling`.

## 2. KDE Plasma (native Linux)

From the repository root (a C compiler is needed: `nix shell nixpkgs#gcc -c`, or `nix develop .#native`):

```sh
cargo check -p komorebi-linux -p komorebi -p komorebic                         # ran, 27 s with a warm target
cargo test  -p komorebi-linux --lib                                            # the daemon's unit tests (294 passed at the step-8 measurement)
cargo test  -p komorebi-linux --test schemas -- --test-threads=1               # the schema oracle; shares a pid-only socket path, keep one thread
cargo test  -p komorebi -p komorebic -p komorebi-client -p komorebi-hotkeys    # daemon-binary, CLI and client tests (tests/linux_*.rs)
```

A single test file: `cargo test -p komorebi-linux --test NAME` (the files are in `crates/komorebi-linux/tests`).
The `-p komorebi --lib` that older notes use is the thin binary package now: the unit tests are in
`komorebi-linux`.

Run `tools/kde-closure-check` (see [tools/README.md](../../tools/README.md)) after any dependency change in the KDE crates or the crates they use: the Linux
source takes only the crates listed in `nix/linux/root-crates.json`.

KDE nix checks (no VM), prefixed `linux-`:

```sh
nix build -L --no-link .#checks.x86_64-linux.linux-clippy
nix build -L --no-link .#checks.x86_64-linux.linux-nextest
# also linux-{fmt,toml-fmt,deny,doc,bridge-js,schema-fresh,nix-options-validation,nixos-module-eval,
# keybindings-fixtures,plasma-guests-eval}
nix build -L .#komorebi .#komorebic            # the native packages of the Linux system
```

`just check` runs `cargo test --workspace`, which would build the Windows crates on Linux. Use the commands
above; `just vm NAME` and `just nixgen-linux` are fine.

### KDE VM suites

Each suite under `nix/linux/tests/vm/suites/NAME` boots NixOS with Plasma and KWin; they need KVM
(nested is enough under WSL2; `nix run .#wsl2-probe` measures it).

```sh
just vm NAME                                              # = nix build -L .#legacyPackages.x86_64-linux.vm.NAME
just vm-all                                               # every suite in the order of the flake
nix build -L .#legacyPackages.x86_64-linux.vm.NAME      # the same suite as a test with a `driver`
# the names of each tier as JSON (ran; CI reads it)
nix eval --json .#legacyPackages.x86_64-linux.vmTiers
```

Tiers (the `tier` in each suite's `default.nix`; CI runs `fast` on every pull request, `slow` on master):

- fast: `check hotplug commands kwin-restart kwin-events scrolling workspace-rules core-e2e kwin-contract kwin
  workspace-switch stacks module placement user-maximize run-kde`
- slow: `client-geometry restart-survival keys-whkd core-lock keys-sxhkd render-smoke drag-resize rdp features
  focus-follows-mouse latency keys-engine live-resize lock-sleep look keys-skhd keys-static wallpaper`
- `vm-lib` (a check, not a suite) runs the unit tests of the VM library; no VM boots.

To run only some steps of a suite: `E2E_ONLY=stacks,monocle nix build --impure .#legacyPackages.x86_64-linux.vm.core-e2e -L`
(a substring of the printed `module.function`; `KFDE_ONLY` is the old name).
To record a run: `KOMOREBI_RECORDER=$(nix build .#komorebi-recorder --no-link --print-out-paths)
nix build --impure .#legacyPackages.x86_64-linux.vm.core-lock -L` (`core-e2e` and
`core-lock` keep `recording-NAME.tar.gz`). Details, the library and how to add a suite:
[docs/linux/dev/vm-tests.md](../linux/dev/vm-tests.md). The suites use a daemon
built once with the constant stamp, so a new commit does not rebuild it.

## 3. macOS

On a Mac (aarch64-darwin), from the repository root (a `.git` checkout, or `COMMIT_HASH=dev` in the
environment). These are the commands for the Mac Mini; none has been run on a Mac against wip-cand4 unless
[macos-handcheck.md](macos-handcheck.md) says so:

```sh
cargo build --locked -p komorebi -p komorebic -p komorebi-bar
cargo clippy --locked -p komorebi -p komorebi-macos -p komorebic -p komorebi-client -p komorebi-bar -p komorebi-bar-core --all-targets -- -D warnings
cargo test  --locked -p komorebi-macos -p komorebic -p komorebi-client -p komorebi-bar-core -p komorebi-bar
cargo build --locked --features deadlock_detection -p komorebi          # the feature is forwarded
target/debug/komorebi --version                                          # komorebi 0.1.0 (the macOS version crate)
target/debug/komorebic --help; target/debug/komorebic docgen --output /tmp/docs-mac
target/debug/komorebi-bar --schema | cmp - schemas/macos/schema.bar.json
```

Never `-p komorebic-macos` together with `-p komorebic` (the shims `komorebic-macos`, `komorebi-client-macos`
and `komorebi-bar-macos` still build alone). Before any change that touches macOS code, run the unit tests on
the Mac first: nothing else can run them. After that walk [macos-handcheck.md](macos-handcheck.md).

On Linux, macOS can only be type-checked, through a fake C compiler and the target
`aarch64-apple-darwin`: `tools/mac-check.sh` does it unconditionally, `tools/fast-check` when macOS files changed
(fake compiler: `tools/cc-fake.sh`); it cannot link or run:

```sh
tools/mac-check.sh     # = COMMIT_HASH=dev cargo check --target aarch64-apple-darwin ... --all-targets, CC set to tools/cc-fake.sh
```

Nix, on a Mac: the `macos-*` checks (`macos-{clippy,fmt,toml-fmt,deny,nextest}`,
`macos-nix-options-validation`, `macos-darwin-module-validation`) and the packages `komorebi`, `komorebic`,
`komorebi-bar` of `aarch64-darwin`. From Linux they can be evaluated only (`nix flake check --no-build
--all-systems`, `nix eval`), which proves nothing builds.

## The docs site (`starlight/`)

The site is an Astro Starlight project, with the pages of Windows and macOS. The guides are written by hand in
`starlight/src/content/docs/`; the reference is generated, and `just docgen` on Windows or macOS writes it in its
final form into the same directory (`komorebic-<platform>.mdx`, `komorebi-<platform>.mdx`, `bar-<platform>.mdx`, and
the page of every command in `starlight/src/data/cli/<platform>/`). Run it again after a change of a command, of its
help or of `schemas/<platform>/`; it needs `node` (in the dev shell) and builds `komorebic` and `schemars-docgen`.

```sh
just docgen                          # the reference pages of this platform
cd starlight && npm ci && npm run dev    # the site on localhost; `npm run build` makes dist/
```

`ci / starlight` runs `npm ci` and `npm run build` when a file of `starlight/` changes. `nix build
.#checkSets.docgen-lint .#checkSets.docgen-test` checks the tool.

## 4. Windows e2e (QEMU guest)

Full text: `e2e/platforms/qemu_windows/README.md` (building the golden image, every variable). Short form:

```sh
E2E_GOLDEN=golden nix run .#e2e-windows -- OUT SCENARIO...      # e.g. OUT tiling focus commands
E2E_GOLDEN=golden KOMOREBI_E2E_SCALE=1.5 nix run .#e2e-windows -- OUT bar   # the komorebi-bar scenario
E2E_GOLDEN=golden KOMOREBI_E2E_STATE=state-nextest nix run .#windows-nextest -- OUT   # the Windows crates' unit tests in the guest
```

The app uses this checkout's binaries, built with the constant stamp `e2e` (so a docs-only commit reuses them);
the commit under test is `KOMOREBI_E2E_COMMIT` (default: the flake revision). To test another commit:
`build-ref.sh REF bins` then `KOMOREBI_E2E_BINS=bins ...` (`bins` needs `komorebi-bar.exe` for `bar`).

| Variable | Effect |
|---|---|
| `E2E_ONLY=word,word` | run only the steps whose printed name contains a word (a rerun of one step) |
| `KOMOREBI_E2E_LOG_LEVEL=trace` | the daemon's log level (`debug`, `trace`) |
| `KOMOREBI_E2E_RECORDER=DIR` | record with komorebi-recorder (a folder with `komorebi-recorder.exe`); comes back as `recording.zip` |
| `KOMOREBI_E2E_KEEP_GUEST=1` | keep the guest's run directory (overlay, TPM state) instead of deleting it |
| `KOMOREBI_E2E_SCALE=1.5` | every wait is 1.5 times as long, for a busy host |
| `KOMOREBI_E2E_STATE=DIR` | restore guests from a saved state (3 s instead of 50-90 s start) |
| `KOMOREBI_E2E_CAPTURE=DIR` | save raw `komorebic state` fixtures |
| `SWEEP_TWO_DISPLAYS=1` | `commands` adds a second display |

Keep output and run directories on a short path (unix socket paths are limited to about 108 bytes). Each
scenario gets its own guest (about 4 vCPU and 6 GB). Results are in `OUT/<scenario>/result.json`; trust it over
the printed line. `docs/dev/bar-e2e.md` describes the `bar` scenario.

A change to the Windows daemon or bar needs these before it is considered landed (commit types BEHAVIOUR):
`commands`, plus the scenario of the cluster named in the revision body (see backend-switch.md for the table).

### The full Windows suite by hand

CI runs, on Windows, only the packages that need Windows: the `windows` and `os_sensitive` classes of
`tools/test-partition.json` (the neutral packages run on Linux and macOS, `crates/komorebi-macos-core` among them). The
whole suite that ran on Windows before the partition is still there, to run by hand:

```sh
just test-windows-full          # on a Windows machine, native: cargo test --workspace (less the other platforms' crates) and the recorder
just test-windows               # on a Windows machine: only the windows and os_sensitive packages, with nextest (what CI runs)
nix build -L --no-link .#checkSets.windows-test-full           # cross-builds the test executables of every Windows crate, runs none
E2E_GOLDEN=golden KOMOREBI_E2E_STATE=state-nextest nix run .#windows-nextest-full -- OUT   # the whole suite in the guest
```

`windows-nextest-full` and its build `windows-nextest-full-build` (the set `windows-test-full`) are not in
`defaultCheckSets` of `nix/checks.nix`, so no pull request builds them; `windows-nextest` and `windows-nextest-build`
are the trimmed default. Both derive their package list from the same `tools/test-partition.json`. The recipes of the
justfile that need the partition call `bash tools/test-partition`, so on Windows they want Git for Windows' bash and `jq`.

## 5. What runs where (summary)

| Change touches | Run |
|---|---|
| `crates/*` (not bar/komorebic) | section 1 native crate tests, then Windows clippy (section 1), KDE (section 2) and macOS type-check (section 3) |
| `crates/komorebi-windows/` (Windows daemon) | Windows clippy and nextest, then e2e (section 4) |
| `crates/komorebi-linux/`, `crates/komorebi-hotkeys/` | section 2 cargo, then KDE checks, VM `fast` tier |
| `crates/komorebi-macos/` | macOS type-check on Linux; build and tests on a Mac |
| `komorebi-bar/`, `komorebi-bar-core` | Windows clippy, `cargo test -p komorebi-bar-core`, macOS type-check, e2e `bar` |
| `komorebic*`, `komorebic-core` | `cargo test -p komorebic-core` (CLI doc oracle), all three platforms' checks |
| `flake.nix`, `nix/**`, `flake.lock` | `nix flake check --no-build --all-systems`, then the checks of the platform edited |
| docs only | nothing, unless under a platform tree (see section 0) |

## 6. Known flakes

Policy: on any e2e or VM failure, check this list by check name and by log signature first; do not soak to
re-prove a known flake. Write rates as `failures/runs` with the commit and host load. A known flake needs
`baseline-seen` evidence (it occurred before the consolidation) or at least 3 occurrences with the same
signature. A name-only match gets one rerun on a calm host at `trace` with logs kept; no match gets one calm
rerun, and a second failure is a regression candidate to bisect by revision. A soak is only for a candidate
that failed twice or a flake whose rate looks more than 2x higher with at least 3 failures. Passing under
load stays a pass; failing under high load is not evidence of a regression. A full `commands` sweep takes 4
to 6 minutes, so the `E2E_ONLY` runs come first.

| Check | Platform | What is seen | Rate (from a measured set) |
|---|---|---|---|
| `commands`, daemon stops answering `komorebic state` | Windows | the daemon logs nothing from any thread for 3 to 3.6 s after answering `State`; the reply write hits a closed socket (`os error 10054`); `border manager: stuck for ~3000 ms in locking the window manager`; `could not acquire window manager lock`. Cause not found (host I/O stalls are a candidate) | 1/10 sweeps before the consolidation, 1/12 after; probe 0.35% vs 0.39% of calls |
| `lifecycle_hidden` (monocle after crash) | Windows | no signature captured (capture at `trace` next time) | 1/10 before, 2/12 after |
| `resize` (drag checks `win2:bottomleft`, `win3:top`) | Windows | drag checks fail, pass on rerun; documented as flaky | 1 failure in the first run, passed twice |
| daemon does not start (`failed call to AllowSetForegroundWindow after 5 retries`) | Windows | the scenario prints "did not run, counted as 1 failed", no `result.json`; only seen at load 6.6-8 | candidate, 4/~45 starts at that load vs 0/~150 at load 3-5 |
| `hotplug` with the recorder on (`AgentLost ... during exec tasklist`) | Windows | after the display unplug, in the recorder collection step; the rerun passed | candidate, 1 occurrence |
| `maximizing.user`, `placement` (integer case), `unknown_monitor` (2) | Windows | not flakes: the same 4 checks fail on every commit (stale known-failure markers) | fix or re-mark them |
| `focus_follows_mouse` "did not run" | Windows | reporting quirk of the runner, `result.json` says 15/0 | ignore the line |
| `vm-commands`, `vm-core-e2e` | KDE | `commands.py` (guest D-Bus stops replying, BrokenPipe), `handover.py` ("win3 is inside the work area and not on a tile"), `lifecycle_hidden.py`, `manage_rules.py` (window never appeared, 30 s), `core_e2e.py` (KWin script never reported in) | 0 green in 6 runs before the consolidation at host load 25 to 190; load-dependent is plausible, not shown; no gate decision until one calm-host run exists |
| `golden` test of `komorebi-protocol` | native | "5 passed; 1 failed" once, six reruns passed, text not captured | 1 occurrence; keep the text if it recurs |
| KDE `tests/schemas.rs` | native | shares a pid-only socket path | keep `--test-threads=1` |

Observed rates are consistent with a wide range: 1 failure in 10 runs fits anything from about 0.3% to 45% per
run. Quote the counts, not "flaky". The source registry of the consolidation runs (with commits and logs) is
kept by whoever runs the gates; add new occurrences there and update this table when a rate changes.
