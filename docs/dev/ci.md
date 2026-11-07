# Continuous integration

Status: written together with the workflows, and none of them has run on GitHub: they were checked with
`actionlint` and `shellcheck`, and every nix attribute they name was evaluated (see "How to verify before merging").
What a hosted runner does (KVM, memory, disk, the macOS image, the Windows ARM image, Cachix) is not known until
the first run.

The workflows are in `.github/workflows/`; the paths that say which platform a change concerns are in
`.github/ci-paths.yaml`; the action that installs nix is `.github/actions/nix`.

## The ladder (`ci.yaml`, named `ci`)

Each stage is cheaper than the next and starts only when the one before it passed, so a typo costs two minutes
and not an hour. A stage that a change does not concern is skipped, and a skipped job does not stop the stages after
it (their conditions start with `!cancelled() && !failure()`). `ci / required` needs every job, and passes when
each is `success` or `skipped`: it is the only check a ruleset needs to require.

| stage | jobs | runner | what it covers | when it runs |
|---|---|---|---|---|
| (gate) | `changes` | ubuntu-24.04 | `dorny/paths-filter` over `.github/ci-paths.yaml`: `linux`, `macos`, `windows` | always; a manual run concerns all platforms |
| 0 | `hygiene` | ubuntu-24.04, `nix develop .#hygiene` | no CRLF; the macros `just lint` bans (`eyre!`, `dbg!`, `println!` in the Linux and macOS daemons); actionlint; shellcheck of `scripts/linux/wsl2-probe.sh` and `tools/*`; `tools/kde-closure-check`; `tools/test-partition check`; `dependencies.json` is what `just depgen` writes | always |
| 0 | `fmt` | ubuntu-24.04, `nix develop .#hygiene` | the nightly rustfmt of the flake over every file (`tools/format-all --check`), all platforms | always |
| 0 | `deny` | ubuntu-24.04, `nix develop .#hygiene` | `cargo deny check` over the workspace | always |
| 1 | `nix-eval` | ubuntu-24.04, nix, swap | `nix flake check --no-build --all-systems` (every output of every system evaluates: the Windows, Linux and macOS packages and checks); builds `checkSets.fmt`, `checkSets.linux-options`, `checkSets.macos-options` (the macOS options and the nix-darwin module, evaluated on Linux), and the Windows e2e Python | always |
| 2 | `lint` (4 legs) | ubuntu-24.04, nix | `checkSets.{windows-lint,linux-lint,recorder-lint,nixgen-lint}`: clippy and rustdoc, Windows by cross build | always |
| 2 | `mac-typecheck` | ubuntu-24.04, nix | `tools/mac-check.sh`: the macOS packages type-checked for `aarch64-apple-darwin`, no link, no run | macOS paths on a pull request; always on a push |
| 3 | `test` (4 legs) | ubuntu-24.04, nix | `checkSets.{linux-test,windows-test,recorder-test,nixgen-test}`: Linux tests, the Windows test executables cross-built (not run), the recorder's and nixgen's tests | always |
| 3 | `windows-native-test` | windows-latest | `cargo nextest` of the `windows` and `os_sensitive` classes of `tools/test-partition` | Windows or shared paths |
| 3 | `windows-native-test-arm` | windows-11-arm | the same on ARM | label `ci:win-arm`; a push to master with Windows paths; by hand |
| 3 | `macos-hosted` | macos-latest (`_macos.yaml`) | the macOS checks, the whole test suite of the recorder, the `os_sensitive` packages of `tools/test-partition` and the tests and clippy of the e2e tools (all in `nix develop .#native`: the default shell builds for Windows), the binaries, the schemas | macOS paths |
| 4 | `discover`, `vm-fast` | ubuntu-24.04, KVM | the fast tier of the KDE VM suites, one machine each (`max-parallel: 8`, 45 minutes), a machine with no KVM drawn again (`_vm.yaml`, see KVM) | Linux paths; label `ci:vm`; every push to master |

Beside the ladder:

| workflow | jobs | runner | what it covers | when it runs |
|---|---|---|---|---|
| `sweeps.yaml` | `vm-slow` | ubuntu-24.04, KVM | the slow tier of the VM suites, 60 minutes each, logs kept 7 days, a machine with no KVM drawn again (see KVM) | label `ci:vm-slow`; every push to master; by hand |
| | `nixos-eval`, `linux-demo` | ubuntu-24.04, swap | every NixOS system of the module and of the Plasma guests (about 11.6 GB to evaluate), and the demo guest | the same |
| | `canary` (3 legs) | ubuntu-24.04, KVM | every check against the tip of nixpkgs-unstable, one leg for the checks, one for each VM tier; it opens an issue when one fails | Saturday 05:20 UTC; by hand |
| `update-flake-lock.yaml` | `update` | ubuntu-24.04 | a pull request that moves `flake.lock` | Monday 03:00 UTC; by hand |
| `windows-guest.yaml` | `guest` | ubuntu-24.04, KVM | EXPERIMENTAL: builds the Windows guest from the evaluation ISO and runs `nix run .#windows-nextest` in it | by hand only |

`ci` runs on a pull request (opened, pushed to, reopened, labeled), in the merge queue, on a push to `master`, and by hand. A
pull request cancels its own earlier run; nothing else cancels. The token of every workflow is read-only (`contents: read`), and
no workflow uses `pull_request_target`: a pull request from a fork gets no secret.


### The tools of stage 0 are the latest, and the flake is how CI gets them

Every step of `hygiene`, `fmt` and `deny` runs in `nix develop .#hygiene` (`nix/common.nix`), not with the
copy of a tool that the runner image or an action ships, so that a local `nix develop .#hygiene` and CI give the
same answer. The policy for those tools is to run the latest release and to change the code to satisfy it, not to
pin an older one or to silence a finding: when a mismatch shows up, move the flake forward first. The shell holds
the toolchain of `rust-toolchain.toml` (stable), the nightly rustfmt, `cargo-deny`, `shellcheck`, `actionlint`, `jq`
and `ripgrep`, and `hygiene` prints their versions at the start of every run, so a stale pin is visible in the log.
When this was written every one of them was the latest upstream release (actionlint 1.7.12, shellcheck 0.11.0,
cargo-deny 0.20.2, ripgrep 15.2.0, jq 1.8.2, cargo 1.99.1) and the nightly rustfmt was the one of 2026-11-04.

They stay current in two ways: the weekly `update-flake-lock` workflow (Mondays) moves every input, and a tool that
is behind the release in between is brought forward by hand with `nix flake update rust-overlay` (the toolchains
and the nightly rustfmt) or `nix flake update nixpkgs` (the rest; this one also moves Plasma, which the VM suites
test, so check them after it). Two tools are not in the shell and keep their own versions: `taiki-e/install-action`
installs nextest on the Windows runners, which have no nix, and `windows.yaml` (the release workflow) runs its own
`cargo-deny` action.

## What is and is not covered

| | Windows | Linux (KDE) | macOS |
|---|---|---|---|
| format | `fmt`, `checkSets.fmt` | the same | the same |
| clippy | `lint` (cross build) | `lint` | `macos-hosted` only (`macos-lint`); `mac-typecheck` type-checks on Linux |
| unit tests | `windows-native-test` on Windows, and `windows.yaml` on every event | `test` (`linux-test`) | `macos-hosted` only (the macOS packages, the `os_sensitive` ones and the whole recorder natively, and the e2e tools) |
| integration | none (the guest is experimental: `windows-guest.yaml`) | `vm-fast`, `vm-slow` | none in CI; by hand on a Mac: `nix run .#e2e-macos` (a Tart guest, `e2e/platforms/tart_macos`) |
| options and schemas | `linux-options`, `macos-options` on Linux; the Windows schemas are checked by the Windows e2e Python | `linux-options` | `macos-options` on Linux; the schemas by `macos-hosted` |

The gaps:

- No Windows e2e: the guest needs the evaluation ISO and about 20 minutes of Windows Setup, and nothing says a hosted runner can do it.
- macOS runs on every change to its paths, on `macos-latest` (Apple silicon, the latest macOS, the only one supported): standard hosted runners are free and
  unlimited on a public repository. Nothing starts the daemon there, so tiling on macOS is not exercised in CI; whether a hosted runner has a GUI session with the
  Accessibility and Screen Recording permissions is not known. The shared e2e scenarios do run on macOS, by hand, on a Mac (`nix run .#e2e-macos`,
  `e2e/platforms/tart_macos/README.md`): a hosted runner is itself a virtual machine and cannot start a macOS guest, and a guest runs on Apple
  hardware only, two at a time, so CI would need a Mac of its own.
- The nightly rustfmt of `fmt` is the nightly of the day; `checkSets.fmt` in stage 1 uses the one the flake pins. When only `fmt` fails, the nightly moved.
- `mac-typecheck` takes the std of `aarch64-apple-darwin` from the toolchain of the runner (`rustup target add`), because the dev shell of the
  flake only has the Windows target. clippy could run the same way (`cargo clippy --target aarch64-apple-darwin`); `tools/mac-check.sh` does not.
- A change that touches no platform path (documentation, `README.md`) still runs stages 0 to 3, because they are cheap with the cache.

## How this lives next to `windows.yaml`

`windows.yaml` is the release workflow of the Windows build and is not touched: it keeps running `cargo test` for every crate of the workspace on
every pull request and push to master (about 40 to 50 Windows minutes per event) and builds and publishes the nightly, the MSI and winget. The new
workflows do not publish anything, use no `write-all`, and use their own concurrency groups and artifact names (`ci-*`).

- `needs:` does not cross workflows, so `windows-native-test` waits for `lint` of `ci`, and not for the build of `windows.yaml`. The two run at the same time.
- `windows.yaml` has no `merge_group` trigger, so a ruleset cannot require it in a merge queue: it would never start. Require `ci / required` only.
- The partition makes `windows-native-test` run fewer tests than `windows.yaml` (`just test-windows-full` and `windows-nextest-full` run all of them by hand). When
  it has been green for a while, the test step of `windows.yaml` can be removed by the owner of that file.
- The partition (`tools/test-partition.json`) puts `komorebi-windows` in the class `windows` and `komorebi-config`, `-protocol`, `-transport` and `komorebic-core` in `os_sensitive`:
  those are what `windows-native-test` runs, and what `macos-hosted` runs on macOS: a test that renders text worded per platform (`docs_cli` of `komorebic-core` checks the pages of `docs/macos/cli`, and
  skips on every host but a Mac) can only pass on the host whose wording it checks, so it is not covered by the `linux-test` run of the same package. `komorebi-client`, `komorebi-bar` and `komorebi-recorder` are `mixed`, so a platform job names them with `-p`: `macos-hosted` runs the whole recorder (its tests are worded in `cfg!(target_os = "macos")` branches, and a Linux or Windows run takes the other side of each; a filter of the macOS modules once selected 36 of 731). The e2e tools (`tool`: the test window and the agent of the guests) have macOS sources that only that job compiles, so it runs their tests and clippy. The nix set `windows-test` builds the
  same subset; the whole suite is the set `windows-test-full` (not in the default sets, so not in `ci`) and `just test-windows-full`.
- Both run `cargo-deny`; `ci / deny` is the cheaper copy and stops the ladder first.
- `actionlint` in `hygiene` leaves `windows.yaml` out: it reads the PowerShell of that file as bash. `just lint` runs it on every workflow and fails on that file.

## Repository settings that the owner has to change

None of these can be set from a workflow.

1. A ruleset on `master` that requires the status check `ci / required` (and nothing else), with the merge queue on if wanted.
2. Settings, Actions, General: the default token permission is read-only; approval to run workflows from outside collaborators (first-time contributors at least).
3. Already created, nothing to do: the public Cachix cache `komorebi` and the variable `CACHIX_CACHE=komorebi`, the secret `CACHIX_AUTH_TOKEN` (read and write), the secret
   `CACHIX_AUTH_TOKEN_RO` and the variable `CACHIX_PUBLIC_KEY`. The workflows use only `CACHIX_CACHE` and `CACHIX_AUTH_TOKEN`: the cache is public, and `cachix/cachix-action`
   configures the substituter and its key with no token at all, so a pull request, a fork and the merge queue read the cache without one and
   `CACHIX_AUTH_TOKEN_RO` and `CACHIX_PUBLIC_KEY` are not needed (they would only be for a hand-written `extra-substituters`). Every job that runs nix goes through
   `.github/actions/nix` (the `update-flake-lock` job too), whose callers pass `vars.CACHIX_CACHE` and `secrets.CACHIX_AUTH_TOKEN` (the reusable `_macos.yaml` gets the secrets
   with `secrets: inherit`). The write token is handed to `cachix-action` (as its `authToken` input) on a `push` (master, and the temporary komomono trigger, which no expression
   names), on `workflow_dispatch` and on `schedule`; on a `pull_request` or in the merge queue it is an empty string and the action logs `Pushing is disabled`. The only job
   that reads and never writes is the nixpkgs canary (`push: "false"`): it builds against another nixpkgs, which nothing else needs. A job that has no `CACHIX_CACHE` builds with no cache.
4. The labels: `ci:vm`, `ci:vm-slow`, `ci:win-arm`.
5. Settings, Actions: allow the actions the workflows use (`cachix/*`, `dorny/paths-filter`, `taiki-e/install-action`, `Swatinem/rust-cache`, `DeterminateSystems/update-flake-lock`,
   `EmbarkStudios/cargo-deny-action`) if the repository restricts them.


## KVM on hosted runners

What GitHub documents: hardware acceleration (KVM) on the Linux runners, with the udev rule that `.github/actions/nix` writes
(changelog of 2024-04-02, and of 2023-02-23 for the first release on larger runners). What it does not document: that every
host of the `ubuntu-24.04` pool has it. The pages "GitHub-hosted runners" and "Larger runners" say nothing about nested
virtualization, and no service level covers it. Nothing in the repository can choose the host: a job is given whichever one the
pool has. In run 42182554717 (all 16 hosts the same image, `ubuntu24/20261101.360`, spread over six Azure regions) 3 of 16 failed
the check of `/dev/kvm`, 13 had it; what is known of those 3 is only that the check failed 42 to 46 ms after the udev trigger, which
cannot tell a missing device from a rule that was not applied yet. The step now logs which of the two it is.

What the repository does:

- `.github/actions/nix`, step "Enable KVM" (the only place that enables it: `enable_kvm` of `cachix/install-nix-action` writes the
  same rule, ignores the outcome, and is turned off): writes the rule, loads `kvm_intel` or `kvm_amd`, reloads and triggers udev,
  waits for `udevadm settle`, polls `/dev/kvm` for 30 seconds for read and write access, and as the last resort sets the mode by
  hand. It prints `ls -l /dev/kvm`, `id`, `systemd-detect-virt`, the count of `vmx`/`svm` flags and the loaded `kvm` modules, before and after.
- `_vm.yaml` calls `_vm-attempt.yaml` up to three times. In the first two a host with no KVM does not fail its leg: the leg
  uploads the artifact `kvm-missing-<run attempt>-<attempt>-<suite>` and ends green without having run the suite, and the next
  attempt (a job `plan` that lists the artifacts) runs those suites again, each on a new machine. In the third, a host with no KVM fails
  the suite. So a suite is run, or the call fails: three draws without KVM are the only way a suite is not run and the check is red for it.
- `vm-fast` (`ci.yaml`) and `vm-slow` (`sweeps.yaml`) are calls of `_vm.yaml`; `required` reads the result of the call. It works on any
  branch and any event, because it is inside the one run: it needs no workflow on the default branch and no rerun. (A rerun of the
  failed jobs of a run cannot be started by the run itself while it is in progress, and a `workflow_run` workflow runs only from the file
  on the default branch; that is why the earlier `rerun-no-kvm.yaml` is gone.)
- Not covered: the three legs of `canary` and `windows-guest.yaml` (one machine each, hours long): a host with no KVM fails the
  step "Enable KVM", with the message `this runner has no usable /dev/kvm`; run them again by hand.

When GitHub gives no KVM at all for a long stretch (every leg of three draws fails, the log of the step says the device is absent
and `systemd-detect-virt` or the `vmx` count says why): the check cannot pass on hosted runners. The ways out are a larger
runner (`runs-on` with a label of a larger-runner group: billed per minute also on a public repository, and nothing in the
documentation promises nested virtualization there either, only that the Android emulator is supported), or a self-hosted x86-64
Linux machine with KVM, with `runs-on` changed in `_vm-attempt.yaml`.

Not verified: none of this has run on GitHub. In particular the artifact listing of `plan`, the nesting of two reusable workflows
and the result of a call whose legs are skipped.

## The `komomono` removal checklist

`ci.yaml` runs on a push to the temporary branch `komomono`, so that the stack can be tried before it is merged to `master`. It
has a line `- komomono` under the comment `# TEMPORARY: remove when komomono is merged to master`. When it is merged:

- [ ] delete the `- komomono` line and its comment in `.github/workflows/ci.yaml`
- [ ] `sweeps.yaml` never runs on that branch (its push trigger is `master` only): nothing to delete there

`grep -rn komomono .github` finds nothing afterwards. No expression in a workflow names the branch: the conditions say "a push" and "a push to master".

## How to verify before merging

Nothing can be run on GitHub from a checkout, so the first run is the test.

1. `nix shell nixpkgs#actionlint nixpkgs#shellcheck -c actionlint` over the workflows other than `windows.yaml`.
2. `nix eval` of every attribute the workflows build, for example `nix eval .#legacyPackages.x86_64-linux.checkSets.linux-lint.name` and `nix eval --json .#legacyPackages.x86_64-linux.vmTiers`;
   `nix flake check --no-build --all-systems`.
3. Push the stack to a scratch branch, add that branch to the `push` list of `ci.yaml` in the scratch branch, and run `ci` with `workflow_dispatch` (all platforms). Then:
   a pull request with no label (stages 0 to 3), one with `ci:vm`, and one with `ci:win-arm`.
4. Break a check on purpose and see that `ci / required` fails; skip a stage with a path and see that it passes.
5. Run `sweeps` and `windows-guest` by hand once.

## Verify the cache after the first push to komomono

Pushes happen in the post step of `cachix-action` (the daemon registers a post-build hook, so only paths the job builds are pushed, not those substituted from cache.nixos.org;
the post step runs when the job fails too).

- [ ] the log of the `Post Run ./.github/actions/nix` step of `hygiene` shows `Cachix: push` and `Pushing` lines (and not `Pushing is disabled`)
- [ ] the Cachix dashboard of `komorebi` lists new store paths and a size
- [ ] a second run of the same commit is faster, and its log shows `copying path ... from 'https://komorebi.cachix.org'`
- [ ] a pull request run of the same jobs shows `Pushing is disabled` and still copies from `komorebi.cachix.org`
- [ ] the size of the cache against the plan of the account: the xwin Windows SDK (over 1 GB) and the NixOS guest closures (about 10 GB each for Plasma) are pushed unfiltered; if the free
  tier is too small, a `pushFilter` on the `cachix-action` step is the lever, at the price of rebuilding what it excludes

## Open items

- Cachix: whether the free tier of the cache `komorebi` is large enough for the store of this flake (several GB per system) is not verified.
- KVM on hosted runners: documented for public repositories; see KVM below: GitHub does not promise it of every host, so the workflows draw again, up to three times in all.
- The macOS image: `macos-latest` (macOS 26 today; `macos-14` was retired on 2 November 2026) is Apple silicon with its own nix install each run; whether the cache makes the checks fit the 2 hours of the job is not known.
- The first run of stage 0 sets up the `hygiene` shell, which is a download of about 1 GB until the Cachix cache has it (a push and a manual run push it).
- The memory of `nix flake check --no-build` over the nixosConfigurations (about 7 GB) against a runner that has 7 GB (private repositories) or 16 GB (public): the swap of the action is the mitigation.
- `windows-native-test-arm` assumes the `windows-11-arm` runner exists for this repository and installs Rust itself when the image has none.
