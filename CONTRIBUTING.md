# Contributing to the Project

The project is a collection of contributions from both the project leaders and
community members. There are many ways to contribute, this can include content
in the project repositories, as well as contributing in public and private
conversation, assisting users, writing blog posts, and many other ways.

## How contributions are made

Contributions to the project primarily happen in the project source
repositories, but may also occur in other places, such as discussion forums and
public and private discourse.

## Contributing content to the Project

In order for the project leaders to manage sustained progress toward the
project goals and maintain project velocity, focus and quality, the project may
adjust the license terms over time.

Content contributed to the project must therefore be provided under
sufficiently liberal terms to allow these operations to proceed unimpeded. As
such contributions are accepted with the following understanding:

* Contributed content is licensed under the terms of the 0-BSD license
* Contributors accept the terms of the project license at the time of
  contribution

By making a contribution, you accept both the current project license terms,
and that all contributions that you have made are provided under the terms of
the 0-BSD license.

## Working on the code

`nix develop` (or [direnv](https://direnv.net), through the [`.envrc`](.envrc))
provides the Rust toolchain, a nightly `rustfmt` and the tools the checks use.
[`rustfmt.toml`](rustfmt.toml) uses an unstable option, which is why the
formatter is a nightly one.

`just check` runs everything a change has to pass before it is pushed: the
formatting check, clippy with warnings denied, the tests, rustdoc with warnings
denied, the checks of the bridge, and `just lint`: the macros the code does not
use, and actionlint (with shellcheck) over the workflows. `just fmt` formats the Rust sources, and `nix fmt` every file
type the repository has. `nix flake check` builds the same checks, plus
`cargo deny`, as derivations; on Linux it also boots every VM test, which needs
KVM.

## The bridge

[`bridge.js`](komorebi/src/kwin/bridge.js), the script komorebi loads into
KWin, is JavaScript that KWin runs as it is: there is no package.json and no
build step. Its types are JSDoc comments, which `tsc` checks against
[`kwin-api.d.ts`](komorebi/src/kwin/kwin-api.d.ts), the part of KWin's
scripting API the bridge uses; something the bridge starts to use is declared
there first. Its tests, [`bridge.test.js`](komorebi/src/kwin/bridge.test.js),
run under `node --test` against a fake KWin. `just check-js` runs both, with the
`tsc` and `node` of the dev shell.

The messages between the bridge and the daemon are in
[`komorebi/src/kwin/fixtures`](komorebi/src/kwin/fixtures), which the tests of
both sides read. A change to a message changes the fixture, the bridge and the
daemon in the same commit.

## The NixOS module

[`nix/nixos-module.nix`](nix/nixos-module.nix) is `services.komorebi`, which the
flake exports as `nixosModules.default` with its overlay. A change to it comes
with a scenario in [`nix/tests/nixos-module.nix`](nix/tests/nixos-module.nix),
the `nixos-module-eval` check: each scenario is a real NixOS system that is
evaluated and never built, and its checks read the assertions, the unit and
the options it evaluates to. It runs on a Mac too, with `nix build
.#checks.aarch64-darwin.nixos-module-eval`. A change to what the unit does in a
session also changes the VM suite `module`, which logs in to Plasma on a machine
with the module and checks the daemon the unit runs. Every other VM suite runs
on machines built with the module too.

Three files are generated from the Rust types and committed: `schema.json` and
`schema.asc.json` (`just jsonschema`), and `nix/komorebi-options.nix`, which types the
module's `config` (`just nixgen`, which needs `schemars-nixgen`, `cargo install
schemars-nixgen`). A change to what `komorebi.json` or an `applications.json` can
say regenerates them in the same commit, and the check `schema-fresh` fails when
it does not. The first line of the options file is the hash of the schema it was
made from, which is how the check tells without running `schemars-nixgen`.

A change to what `komorebic hotkeys compile` prints for a key binding file shows
in `keybindings-fixtures`, which compares its output for every file of the
fixture corpus with `nix/tests/keybindings.expected`: write the file again with
the loop in `nix/tests/keybindings.nix`, and read the difference before
committing it.

[`nix/supported-kwin.nix`](nix/supported-kwin.nix) has the oldest KWin komorebi
supports, `min`, and the newest one every VM suite has passed against,
`testedMax`, which the module checks a system's KWin against. `testedMax` is
raised by hand, once every suite has passed against a newer KWin: in the weekly
canary job, or on a branch whose `flake.lock` has it. The commit changes
`testedMax` and the dated comment above it (the date, the nixpkgs revision and
the KWin), and its message says which run that was and which suites ran.
`nix eval --raw --file nix/supported-kwin.nix testedMax` reads it without the
flake. `min` changes with the code, in the commit that starts to use something an
older KWin does not have.

## The comparison

[`docs/dev/COMPARE.md`](docs/dev/COMPARE.md) compares komorebi for KDE with komorebi
for Windows and komorebi for Mac, and its tables are generated by
[`scripts/compare-gen.py`](scripts/compare-gen.py) (Python 3 and nothing else) from
this checkout and from checkouts of the other two, `~/komorebi` and
`~/komorebi-for-mac` unless `--kfw` and `--kfm` say otherwise. Continuous
integration has neither checkout, so it does not run the script. A change to a
`komorebic` command, a `SocketMessage`, a key of `komorebi.json`, a
`WindowManagerEvent`, a method of `Backend` or a list of keys in `check.rs` is
followed by `python3 -I scripts/compare-gen.py --check docs/dev/COMPARE.md`, and
by `--update` when it reports a stale block, in the same commit. The script stops
and says what when a command, key, event or method has no note about why it
differs, and when a sentence of the document stops being true; the text around an
updated block is read again each time.

## VM tests

A change that needs a real KWin to be checked comes with a VM test, next to
the Rust test that covers the same behavior against the fake desktop.
[`docs/dev/vm-tests.md`](docs/dev/vm-tests.md) says how they are laid out, how
to run one (`just vm NAME`) and how to add one.

## The KWin effects

The patches in [`nix`](nix) change the KWin effects komorebi drives. Each one
is a derivative work of its effect, which is under the GNU General Public
License version 3, and is under the same license. A patch starts with a header
that describes the change and carries the license notice; `patch` skips the
text before the first `diff` line. To change a patch, make the change in the
effect's source at the version the package pins, and write the whole diff again
with `diff -ruN a b` below the header. The check of each package, named after
it (in [`nix/tests/effects.nix`](nix/tests/effects.nix)), builds the package
and checks the files it installs, and for a patched package that the patch is
in them:

```sh
nix build .#checks.x86_64-linux.kde-rounded-corners-komorebi-install
nix build .#checks.x86_64-linux.kwin-effect-geometry-change-komorebi-install
```

## Plasma guests to develop in

[`nix/dev`](nix/dev) has NixOS guests with Plasma for trying a change by hand,
on an Apple silicon Mac: two that QEMU runs and a disk image for UTM.
[`docs/dev/utm.md`](docs/dev/utm.md) says how to build and run them, and what a
builder has to be. They install komorebi with the NixOS module, and carry
nothing of the person who uses them: a key, a password or a configuration of
yours goes into the guest when it runs, never into these files. The
`plasma-guests-eval` check evaluates them, and the VM suite `rdp` runs their
RDP server, [`nix/dev/krdp.nix`](nix/dev/krdp.nix).

## Continuous integration

[`ci.yaml`](.github/workflows/ci.yaml) runs on every pull request and on every
push to `master`, `feature/*` and `hotfix/*`, in this repository only. The
`lint` job builds every check that needs no virtual machine and runs
`just lint`, `cargo-deny` checks licenses and advisories, and
the VM tests run on KVM in two jobs: `vm-fast` on every pull request and
`vm-slow` on `master`, every week, and on pull requests labeled `ci:vm-slow`. A weekly
[workflow](.github/workflows/update-flake-lock.yaml) opens a pull request that
moves `flake.lock`, and the same jobs run on it.

The flake follows the tip of nixpkgs-unstable, so KWin and Plasma change
whenever `flake.lock` moves. The `canary` job of `ci.yaml` runs every week, and
when the workflow is started by hand: it runs `nix flake update nixpkgs` in a
copy of the checkout and builds every check against the result, the VM suites
of both tiers included. When a check fails, it opens an issue titled "Canary: a
check fails against the tip of nixpkgs-unstable" with the failed checks, the
nixpkgs revision and its KWin version, or comments on that issue if it is
still open. The job changes nothing in the repository. To reproduce a failure,
run `nix flake update nixpkgs` in a checkout and build the check the issue
names (`just vm NAME` for a VM suite). The fix goes in a commit of its own;
nixpkgs is not pinned back.

## Releases

A release is a tag. Set `version` under `[workspace.package]` in
[`Cargo.toml`](Cargo.toml) to the new version, commit it, and push the tag
`vVERSION` on that commit. For every run, the `build` job of `ci.yaml` builds
`komorebi` and `komorebic` with nix on an x86_64 and an arm64 runner, and keeps
them as an artifact of the run for 14 days. On a tag, once `lint`, `vm-fast` and
`build` have passed, the `publish` job makes the GitHub release of that tag
with:

- `komorebi-VERSION-x86_64-unknown-linux-gnu.zip` and
  `komorebi-VERSION-aarch64-unknown-linux-gnu.zip`, each with the two binaries;
- `checksums.txt`, the SHA-256 sum of every other file of the release;
- `schema.json`, the schema of `komorebi.json`, when the repository has one;
- release notes that [kokai](https://github.com/Joxit/kokai) writes from the
  commit messages since the previous version tag.

The scheduled run on `master` every week, and a run of the workflow started by
hand on `master`, replace the `nightly` prerelease the same way: `nightly`
takes the place of the version in the file names, and the notes list the
commits since the last version tag. The arm64 job may fail without failing the
run; the release then has no arm64 zip.

nix builds a binary that loads glibc from the nix store, so the build job
points each binary at the system's dynamic loader, at the standard path for its
architecture, with no library path, and runs it on the Ubuntu runner to check
that it starts. A release binary therefore uses the glibc of the system it runs
on.

## Commits

Commit messages follow the
[Conventional Commits](https://www.conventionalcommits.org/) specification:
`type(scope): description`. The scope is the Rust module the change is in
(`wm`, `kwin`, `layouts`), `repo` for repository files and markdown, `nix` for
the flake and `github` for workflows. Every line of a message, the header
included, is at most 80 columns, and the body says why the change is made.

Documentation lands in the commit that changes the behavior it describes, not
in a later one. A change to a `komorebic` subcommand regenerates its page in
[`docs/cli`](docs/cli) with `just docgen`.

## Zero-Clause BSD

```text
Permission to use, copy, modify, and/or distribute this software for  
any purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED “AS IS” AND THE AUTHOR DISCLAIMS ALL  
WARRANTIES WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES  
OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE  
FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY  
DAMAGES WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN  
AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT  
OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```