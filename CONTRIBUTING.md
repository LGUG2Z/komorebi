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