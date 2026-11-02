# The checks of the flake are separate sets

Design note for the checks of the one root `flake.nix` (the three platforms). Measurements were made on
the work-in-progress flake of `wip-cutover-flake` (a copy of its working tree, taken 2026-11-01 09:10; the first table ran under a 20 GB cap, its largest peak 10.3 GB, and the later ones under 12 GB),
x86_64-linux, Lix 2.94, `--no-eval-cache`, each evaluation alone in a `systemd-run --user --scope`
with a memory cap; the memory is the cgroup's `memory.peak`, which includes page cache of the store
reads, so it is an upper bound of the evaluator's heap. Numbers marked (inferred) were not run.

## What the checks are

At `wip-main` the three flakes had these checks (x86_64-linux): root (Windows) `komorebi-workspace-{clippy,
deny,fmt,nextest,toml-fmt}`, `e2e-tests`, `pre-commit`, `treefmt`; the nested KDE flake the same
`komorebi-workspace-*` plus `doc`, `komorebi-bridge-js`, `keybindings-fixtures`, `nix-options-validation`,
`nixos-module-eval`, `plasma-guests-eval`, `schema-fresh`, `run-kde*`, `wsl2-probe*`, the two effect
install checks and 35 `vm-*` (34 suites + `vm-lib`; 16 of tier `fast`, 18 of tier `slow`); the nested Mac
flake `darwin-module-validation`, `nix-options-validation` and `komorebi-workspace-*`. The root flake
re-exported the KDE and Mac checks as `kfde-*` and `kfm-*` but not the 34 VM suites. The one flake has
every one of them under platform-prefixed names (`windows-*`, `linux-*`, `macos-*`, `vm-*`): 62 checks on
x86_64-linux, 28 on aarch64-darwin, 9 on x86_64-darwin (`nix eval .#checks.<system>`).

## Why evaluation needed more than 28 GB

Cost of evaluating `checks.x86_64-linux.<name>.drvPath` alone (peak, wall time):

| check | peak | time |
| --- | --- | --- |
| every Rust check, fmt, deny, bridge-js, schema-fresh, options-validation, keybindings, vm-lib, treefmt | 0.14-0.25 GB | 1-3 s |
| pre-commit, wsl2-probe, effect install checks | 0.4-0.5 GB | 3-4 s |
| linux-run-kde, -doctor, -dry-run (they instantiate the demo guest) | 1.5 GB | 17 s |
| one VM suite (`vm-kwin`, `vm-module`) | 1.0 GB | 12.5 s |
| 2 / 4 / 8 VM suites in one evaluation | 1.6 / 2.7 / 5.2 GB | 29 / 64 / 83 s |
| linux-nixos-module-eval (26 `nixosSystem`s) | 10.3 GB | 138 s |
| linux-plasma-guests-eval (9 guests) | 9.0 GB | 107 s |

So the memory is linear in the number of NixOS systems an evaluation instantiates, about 0.55 GB per VM
suite and 0.35-0.4 GB per `nixosSystem` of the module check, and nothing is shared between them. A flake
check holds every check's value, so the total is the sum: 34 suites (about 19 GB) + nixos-module-eval
(10 GB) + plasma-guests-eval (9 GB) + the nine `nixosConfigurations`, which `nix flake check` also
evaluates (7 GB when forced alone, measured), about 47 GB
(sum of those figures) against a host with 31 GB. The nested KDE flake needed 15-16 GB for one system because the
same sum was taken before the suites were 34.

Where the 0.55 GB of one suite goes (each row is an `nix eval` of a stand-alone expression, 4 copies
against 1, the difference divided by 3):

| what is evaluated | per copy |
| --- | --- |
| `nixosSystem` with Plasma, no test framework | about 0.2 GB |
| `testers.runNixOSTest`, Plasma node, framework's shared pkgs | 0.14 GB |
| the same with `node.pkgsReadOnly = false` and an overlay (as the suites need) | 0.18 GB |
| the same, importing `nixosModules.forVmTests` and enabling `services.komorebi` | 0.40 GB |
| the real suite (its node files, the library, the script) | 0.55 GB |

The module costs most: its options file (`nix/linux/komorebi-options.nix`, 1351 lines of typed
submodules) is evaluated once per NixOS system, and its overlay (`overlays.linux`, `overlays.forVmTests`
which applies the first as well) ran `mkPkgs` (a new `import nixpkgs` with the rust overlay) and the whole
crane `mkKomorebiPackages` once per node, twice. Doing that once per system (a `genAttrs` of the systems,
shared by every overlay application; tried on a scratch copy, not committed) lowered the 4-suite
stand-alone expression from 2.15 to 1.50 GB, 8 real suites from 5.18 to 4.79 GB and
linux-nixos-module-eval from 10.3 to 8.5 GB. That is worth doing (D-checks-3) and does not change the
picture: 34 + 26 + 9 + 9 full NixOS evaluations cost tens of GB however they are written, because each is a
separate module-system evaluation whose thunks stay reachable from the check attribute.

`--all-systems` multiplies by the systems that have the checks (three here, each with the Windows and
platform checks; the VM suites exist on the two Linux systems), so it is not the cheap way to see a
problem either.

## The sets

A check keeps its name (`linux-clippy`), so no command that names a check changes. A SET is a list of
members, and every set is also one derivation (`pkgs.linkFarm`) that builds its members:

| set | members (x86_64-linux names) | evaluation |
| --- | --- | --- |
| `fmt` | `treefmt`, `pre-commit`, `windows-fmt`, `windows-toml-fmt`, `linux-fmt`, `linux-toml-fmt` (macOS systems: `macos-fmt`, `macos-toml-fmt`) | cheap |
| `deny` | `windows-deny`, `linux-deny` (`macos-deny`) | cheap |
| `windows-lint` | `windows-clippy` | cheap |
| `windows-test` | `windows-e2e-tests`, `windows-nextest-build` (the Windows test executables, cross-built and not run; they run in the guest with `nix run .#windows-nextest`, below) | cheap |
| `linux-lint` | `linux-clippy`, `linux-doc` | cheap |
| `linux-test` | `linux-nextest`, `linux-bridge-js`, `linux-keybindings-fixtures`, `vm-lib`, `linux-wsl2-probe`, `linux-wsl2-probe-dry-run`, the two effect install checks | cheap |
| `linux-demo` | `linux-run-kde`, `linux-run-kde-doctor`, `linux-run-kde-dry-run` | 1.5 GB |
| `linux-options` | `linux-nix-options-validation`, `linux-schema-fresh` | cheap |
| `linux-nixos-eval` | `linux-nixos-module-eval`, `linux-plasma-guests-eval` | 19 GB |
| `macos-lint`, `macos-test`, `macos-options` | `macos-clippy`; `macos-nextest`; `macos-nix-options-validation`, `macos-darwin-module-validation` | cheap |
| `vm-fast` | the suites whose `tier` is `fast` (16), computed from `suite.tier` | 9 GB |
| `vm-slow` | the suites of tier `slow` (18) | 10 GB |

The Windows end-to-end run is an app (`nix run .#e2e-windows`), and so is `windows-nextest`, not a check: it needs a golden image. The
sets that exist on a system are the ones with members on it (a Mac has no `windows-*` build host except
Apple silicon's cross build, which the member lists already say).

Two more sets are made of the others: `cheap` is the sets of `defaultCheckSets` (what `checks` has, as one
derivation), and `all` is every check. A check that is in no set, or in two, fails the evaluation of `checks`
(`nix/checks.nix`), so a new check has to be placed.

## How they are exposed

`nix/checks.nix` is the one place that knows the sets. Each platform module only says which derivations it
has, as `allChecks.<name>` (a per-system flake-parts option, an attrset of derivations that merges across the
platform modules; `nix/common.nix` adds treefmt and pre-commit), and `nix/checks.nix` does the rest with plain
functions:

```nix
# nix/checks.nix (shape)
sets = { fmt = [ "treefmt" ... ]; deny = [ ... ]; ... };   # the names of each set; vm-fast and vm-slow from the tiers
defaultCheckSets = [ "fmt" "deny" "windows-lint" "windows-test" "linux-lint" "linux-test"
                     "linux-options" "macos-lint" "macos-test" "macos-options" ];   # the one list to edit
checks = members of the sets of defaultCheckSets;
legacyPackages.checkMembers = members;      # nix build .#checkMembers.vm-fast.vm-kwin
legacyPackages.checkSets    = mapAttrs (set: ms: pkgs.linkFarm "checks-${set}" ...) (members // { cheap; all; });
```

* One set alone: `nix build -L .#checkSets.linux-lint`. Only that set's members are evaluated: sets are
  attributes of `legacyPackages`, which `nix flake check` does not walk, and a set that is not named is a
  thunk nothing forces.
* One check alone, as now: `nix build -L .#checks.x86_64-linux.linux-clippy` for a check of the default
  sets and `.#checkMembers.<set>.<name>` for any member (a heavy one is not in `checks`).
* The default composition, `nix flake check` and CI's pull request run: `defaultCheckSets`. Choosing what
  `nix flake check` runs is an edit of that list.
* Everything: `nix build -L .#checkSets.all` (cheap sets and heavy ones; the VM suites need KVM and hours).
  One set of sets: `nix build .#checkSets.cheap`.
* The existing `legacyPackages.<system>.vm`, `vmTiers`, `vmGuests`, `vmTcg` stay (the Mac and CI use them).

`nixosConfigurations` (the nine Plasma guests) is the other thing a flake check could evaluate: forced alone,
their `toplevel` costs 7.0 GB and 76 s, but `nix flake check` of the whole flake peaks at 4.5 GB, so it does not
force them that deeply and they stay where they are.

## Alternatives considered

* A second flake in `nix/checks/flake.nix` with this flake as an input: the evaluation of the checks
  would be separate by construction, but `self` becomes an input with a lock entry that has to be updated
  (`--override-input` for the local tree) and every check would be a different attribute path than the
  one the CI and the docs use. More machinery for what one `defaultCheckSets` list does.
* `--override-input`: it replaces an input, not the set of outputs; it cannot choose checks.
* A `checkSets` function of flake-parts options (the one chosen) versus a templating generator: rejected
  by the brief; the sets are attrsets and `linkFarm`.
* Keeping flake-one's `legacyPackages.<system>.heavyChecks.<name>`: a single bucket with no way to build
  "the fast VM suites" or "the options" alone; the sets replace it and keep its property (nothing under it is
  evaluated by `nix flake check`).
* Lowering the cost of one NixOS evaluation instead of not evaluating it: done where cheap (the shared
  `mkPkgs`/crane per system), but the cost is the module system's, so it is not a substitute.

## Measured with the sets (x86_64-linux unless said, one evaluation at a time, 12 GB cap, 31 GB host)

`nix flake check --no-build` before the sets (the tree of `wip-cutover-flake`, where the heavy checks were
already out of `checks` as `legacyPackages.<system>.heavyChecks`) and after:

| | peak | time |
| --- | --- | --- |
| before, this system | 4.54 GB | 32 s |
| after, this system | 4.54 GB | 33 s |
| before, `--all-systems` | 6.56 GB | 45 s |
| after, `--all-systems` | 6.55 GB | 40 s |

The sets do not make `nix flake check` cheaper than flake-one's split did (they are the same checks); what
they add is that every heavy group is a set that can be built alone, and the default is one edited list.
The first measurement before the split, killed at 28 GB, was not repeated.

Evaluating one set alone (`nix eval .#checkSets.<set>.drvPath`, peak / time):

| set | peak | time |
| --- | --- | --- |
| fmt, deny, windows-lint, windows-test, linux-lint, linux-options | 0.19-0.40 GB | 2-4 s |
| linux-test | 0.54 GB | 6 s |
| cheap (the ten default sets; same on aarch64-linux and aarch64-darwin) | 0.55 GB | 10 s |
| linux-demo | 1.46 GB | 20 s |
| vm-fast (16 suites) | 8.6 GB | 109 s |
| vm-slow (18 suites) | 9.7 GB | 127 s |
| linux-nixos-eval (35 NixOS systems) | 11.6 GB | 191 s |
| macos-lint, macos-test, macos-options (aarch64-darwin); `all` on x86_64-darwin | 0.26-0.53 GB | 2-6 s |

With the per-system sharing of nixpkgs and the crane build (D-checks-3) `linux-nixos-eval` fits in 12 GB
where its two checks took 10.3 + 9.0 GB alone before. Every set of one platform is far below the cap, and no
set was killed.

Nothing is lost: the check names of every system (`checks` plus what `heavyChecks` had) before, and the union
of `checkMembers` after, are the same sets except `windows-nextest` (below), which is now an app, and the new check
`windows-nextest-build`: that was 61 names on x86_64-linux and aarch64-linux, 27 on aarch64-darwin, 9 on
x86_64-darwin, and each system that builds the Windows crates has one more now (not re-counted). The `drvPath` of every one of them is the same before and
after, except `treefmt` and `pre-commit`, which hash the whole source tree and so change with any edit of it.

## `windows-nextest` and `macos-nextest` ran no test

`cargoNextest` of crane runs the tests in its check phase, and the Windows and macOS `individualCrateArgs`
have `doCheck = false` (Linux passes `doCheck = true`), so the derivation ran `cargo nextest --version` and
nothing else. A Windows program's tests cannot run on a Linux builder, so the check `windows-nextest` was
removed (a check that runs nothing under the name of one that runs tests is worse than no check). The tests
now run in the Windows e2e guest, which is the answer to "can the Windows nextest run in a VM from NixOS":

- `windows-nextest-build` (a check, set `windows-test`) cross-compiles every test executable of the Windows
  workspace (`cargo test --workspace --release --no-run`, xwin, 48 executables) and keeps them, so that they
  are proved to compile and link; about 90 s with warm dependencies.
- `windows-nextest` (an app, the name of the old check) boots the golden image like a scenario, uploads the
  executables and the data files they read at the path the build baked in (the schemas, the fixtures), runs
  every test in a process of its own (as nextest does) and prints a line per executable and the failures;
  `OUT/cargo-tests.json` has every result. The guest needs KVM and the golden image, so it is not a check:

  ```sh
  E2E_GOLDEN=golden KOMOREBI_E2E_SCALE=1.5 KOMOREBI_E2E_STATE=state-nextest nix run .#windows-nextest -- OUT
  ```

  964 tests ran and passed (the first run also saves the guest state, 163 s; after that about 3.5 minutes,
  of which the tests are 171 s). It is a release build: debug assertions are off, as in `linux-nextest`.

The neutral crates (`komorebi-layouts`, `komorebi-scrolling`, `komorebi-themes`, the shared `crates/*`) now
also run in `linux-nextest` (1532 tests with the Linux packages), which needs the schemas of every platform
and `docs/macos/cli` in the Linux source (`nix/workspace-src.nix`). `macos-nextest` is the same shape as the
old Windows one and is kept, unchanged and still named for what it does not do, only until a Mac run decides
whether `doCheck = true` is right there (the tests of `komorebi-macos` run natively on an Apple machine).
