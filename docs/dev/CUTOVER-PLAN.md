# The plan to land the consolidated repository and release 0.2.0

Status: a PLAN, written 2026-11-01 by the agent `merge-plan`. Nothing in it has been executed. Sources were
read, nothing was run except read-only commands: `jj log`/`jj file show`/`jj file annotate`, `git log`/`git grep`/
`git blame`/`git show` on the local clone, `gh repo view` and `gh api` GET calls, `gh issue/pr/release/workflow list`,
and one `curl -I` (a HEAD request to a public schema URL, section 2). No branch, tag, issue, release, secret or setting was
changed, and no file outside this document was written.

Everything here is tagged with where it was found (a file and line, or the command) or marked **(inferred)** when it
is reasoning rather than something read. **(unknown)** means it could not be found from here. The cutover
stack read for the first version was `wip-cutover-docs` (`2185556efba6`); this version is on top of **`wip-main` = `6067938e`**, which now contains the
cutover stack and **the one root flake** (docs/dev/flake.md, docs/dev/flake-checks.md): the former `FLAKE_*` placeholders in the workflow drafts are
replaced by the real output names (checks `.#checkSets.<set>`, packages `windows-komorebi*` and native `komorebi*`, VM suites `legacyPackages.<system>.vm.<name>`). File and line references to
the first version still name `2185556efba6`; paths of the flake files are those of `wip-main`.

**The token.** The origin URL of `~/komorebi-for-mac` embeds a GitHub token (seen in `git remote -v`; not copied here). **Owner decision (2026-11-01): it is not rotated; it is how the owner develops locally.** What the plan must guarantee instead is that it is in nothing that is pushed. Checked without ever reading the value: `gitleaks` (default rules, which include the GitHub token formats) found no leak in all 7609 commits of every ref of the work clone nor in the files of any workspace, and the work clone has no remote URL with credentials. Not checked: the literal value (tooling refused to copy a credential into a file). The owner can run the literal check themselves: `T=$(git -C ~/komorebi-for-mac config remote.origin.url | sed -n 's#^[a-z]*://[^:@/]*:\{0,1\}\([^@/]*\)@.*#\1#p'); git -C <work clone> log --all --oneline -S"$T" | wc -l` (expect 0). Anything that has read that clone's config, or this machine's shell history, still has it.
The same URL form is used in `docs/dev/old-ci/macos/workflows/nix.yaml` only as `secrets.GH_USER_API_TOKEN`, which is a
secret name, not a value.

**Owner decisions taken after the first version of this plan (2026-11-01, relayed by the coordinator; they override
the first version where they differ):**
1. **No private sponsors repository.** There will be no private mirror (`KomoCorp/komorebi` or any other) and no
   sponsors-only early access. The history of the old private repositories becomes public with the push, once the owner
   confirms the push itself (step S4). R1 is therefore answered as to the mirror; the confirmation of the push stays a gate.
2. **Schemas are per-platform files in the repository.** Done: `schemas/{windows,linux,macos}/{schema.json,schema.asc.json,schema.bar.json}`
   (linux has no bar schema). R5 is decided (no root copies either: owner, later message); only the published file names stay open.
3. **0.2.0 is not published now.** The workspace version is `0.2.0-dev` (as told; the first version of this plan read `0.2.0` at
   `Cargo.toml:61` of `2185556efba6`; re-read it on the final tip). The landing of the line is the beginning of the
   0.2.0-dev line: `komomono` to `master` carrying `0.2.0-dev`, no tag, no release artefacts, CI on. The later **release** of 0.2.0
   (a revision that sets `0.2.0`, tag `v0.2.0`, artefacts, announcement) is a separate step with its own gates. The macOS hand-check
   gates the release, not the dev-line landing.
4. **One sponsor-only `[FEAT]` policy and one licence for all three platforms (R6), and no root `schema*.json` copies (R5).** The texts
   that describe the policy must be rewritten for one public project with no private tier (task list 2.2a); the old Windows `master/` schema URLs
   stop resolving (2.2b: README and release-notes wording). The published per-platform schema file names stay open until the release.
Sections 4 and 5 and the checklists are split accordingly: **DEV-LINE LANDING** (steps L0 to L6) and **RELEASE** (steps
5. **R1 refined (later message): "yes, but I will push myself".** Publishing the KDE and macOS history is approved. **The owner performs every push and tag command**;
   agents prepare them (5.3) and never run them. Secret scan: see the fact row in section 0 (gitleaks, default rules, 2832 commits, no leaks; its limits are listed there).
6. **R6 follow-up: "you can update these".** An agent, `sponsor-texts`, rewrote the sponsor-only `[FEAT]` texts of 2.2a: **done** (revisions on the dev line, the owner answered its three OWNER notes: sponsors get what the README lists, no private macOS repository, the macOS nightly survives from the joined repository).
7. **R8: "master is already frozen"** (done by the owner): the "freeze" step is removed; merging the ready PRs, the path-map comments and closing the 8 dependabot PRs stay as steps (L1).
8. **R9: "I will self host but this will come later".** The first CI is hosted Linux plus hosted macOS only (the cheap check set `checkSets.cheap` is the PR gate). The Windows e2e, the KDE VM tiers and the canary are a **later phase** on a self-hosted KVM runner, nightly or manual, with the evaluation-ISO caveat (3.3, A.2).
9. **R10: "PR only is good"**: the `hotfix/*` and `feature/*` push triggers are dropped in the drafts. **R12: "GitHub cache sounds good"**: `actions/cache` with per-platform keys, no Cachix for now.
10. **R2 (later message): "macOS will be in 0.2.0-dev; I'll test it and only release 0.2.0 when it's ready".** macOS IS part of the dev line. The macOS hand-check gates only the 0.2.0 RELEASE, which the owner decides when ready; the earlier idea of shipping macOS as a later pre-release is dropped.
   **R3:** Starlight is the site for all docs; `gh-pages` is deprecated (4.9). **R4:** known failures do not block the dev-line landing; the release question is re-opened by the owner. **R5:** published schema names agreed (6). **R7:** the owner handles the old macOS repository. **R11:** removed, there is no upstream nixpkgs package. **N1:** in progress, not decided.
   Sponsors: no private macOS repository remains; the macOS nightly survives from the joined repository (4.8; not in the first CI).
E1 to E8). Where the text below says "S4/S5/S7/S8" the new names are: S4 = L3, S5 = L4, S6 = L5, S7 = E3, S8 = E5.

## 0. What is where today (the facts the rest depends on)

| Fact | Source |
|---|---|
| `LGUG2Z/komorebi` (Windows): public, default branch `master` (`72aa4c97`), 15,331 stars, 376 forks, 755 issues ever (89 open), 858 PRs ever (27 open: 19 not by dependabot, 8 dependabot), no branch protection, no rulesets, GitHub Pages from branch `gh-pages` (`https://lgug2z.github.io/komorebi/`), 30 branches | `gh repo view`, `gh issue list --state open/all`, `gh pr list`, `gh api repos/LGUG2Z/komorebi/branches/master/protection` (404 "disabled"), `.../rulesets` (0), `.../pages` |
| `LGUG2Z/komorebi-for-kde`: **private**, default `master` = `7d19fd40`, 0 stars, 0 forks, 1 open issue (the auto-opened "Canary: a check fails against the tip of nixpkgs-unstable", #6), 5 PRs ever (0 open), 0 releases, 0 tags, workflows `ci`, `rerun-no-kvm`, `update-flake-lock` | `gh api repos/.../komorebi-for-kde` (`visibility: private`), `gh issue list`, `gh release list` (empty), `git tag` (0) |
| `LGUG2Z/komorebi-for-mac`: public, default `master` = `2cd5884f`, **issues disabled**, 261 stars, 4 forks, 0 PRs ever, releases v0.1.0-prerelease, v0.1.0, v0.1.1 (v0.1.1 `Latest`, 2026-05-03), homepage `https://komorebi.lgug2z.com`; the merged macOS history is the branch `ash-squashem` = `9a58e511`, 41 commits ahead of that `master` | `gh repo view`, `gh release list`, `git rev-list --count master..ash-squashem` in `~/komorebi-for-mac`, `gh api .../branches` |
| `KomoCorp/komorebi-for-mac` exists and is private (visible to `gh` as `.private == true`); it is where the macOS CI ran and where the macOS development happened (docs/macos/README.md:43-47). It is retired by the owner's decision: nothing replaces it, and its README text must go (class C in 2.3) | `gh api repos/KomoCorp/komorebi-for-mac --jq .private`; docs/macos/README.md:43-47 |
| The macOS CI runs only in that private repo on a **self-hosted** runner; in the public repo every run is `skipped` (the job guard is `github.repository == 'KomoCorp/komorebi-for-mac'`) | docs/dev/old-ci/macos/workflows/nix.yaml:21-22; `gh api .../komorebi-for-mac/actions/runs` (all `skipped`) |
| `komomono` is already on GitHub: `LGUG2Z/komorebi` branch `komomono` = `ff3711dc` (committed 2026-10-31T01:59Z), 85 commits ahead of `master`; `next` = `72aa4c97` (= `master`) | `gh api repos/LGUG2Z/komorebi/branches/komomono`, `git -C ~/komorebi log --oneline master..komomono` (85), `git merge-base master komomono` (= `72aa4c97`) |
| The local line: **`wip-main` = `6067938e`** (41 revisions on `21cf0d73`: the cutover stack with the renames, the version, the one flake with `checkSets`, the docs; `jj log -r '21cf0d73..wip-main'`), `komomono@source` = `ff3711dc` is its ancestor, **`jj log -r 'wip-main..komomono@source'` is empty and `komomono@source..wip-main` has 1151 revisions** (counted now; it was 1110 at `21cf0d73`) | `jj log` revset counts |
| Secret scan: a `gitleaks` scan of all 2832 commits reachable from `wip-main` found **no leaks with the default rules** (as told by the coordinator; not re-run here). Limits: default rules only; it does not look for private hostnames, internal URLs or e-mail addresses (all commit authors are `LGUG2Z@fastmail.com` plus one dependabot address, from `git log --format=%ae` on the two old clones), nor for secrets in a form the default rules do not know. A custom pass for `KomoCorp`, internal hostnames and the token form of the macOS origin URL is the one cheap addition (the plan's L3 checklist) | coordinator message; `git log --format=%ae` |
| **Superseded:** the master plan's "local `3127741c` vs `komomono@origin` `989a2d4a` (reconcile first)" no longer applies: both sides are `ff3711dc` and the local line is a pure descendant | plan generic-percolating-matsumoto.md (tips read 2026-10-30) vs the commands above |
| History is whole: the old tips (KDE `7d19fd40`, macOS `9a58e511`, Windows `72aa4c97`) are ancestors; commit counts preserved (KDE 254, macOS 304) | progress log (entry of the A2 merge: "old tips resolve and are ancestors", "commit counts preserved (KDE 254, macOS 304)"); `git cat-file`/`merge-base --is-ancestor` of the macOS tags below |
| Tags: Windows `v0.1.0`..`v0.1.41` plus `nightly` (43 tags, all lightweight: `git for-each-ref` reports object type `commit` for each); macOS `v0.1.0-prerelease`, `v0.1.0`, `v0.1.1`, `nightly`; KDE none. The macOS names `v0.1.0` and `v0.1.1` COLLIDE with Windows tags of the same names on different commits; the work clone has only the 43 Windows tags (macOS tags were not imported) but the macOS tag commits (`64341e29`, `ebbd9eee`, `0cc4ca7f`) are ancestors of the cutover stack | `git tag` in each clone; `git merge-base --is-ancestor` |
| Windows 0.1.41 downloads (a proxy for users, not a user count): x86_64 msi 47,170, x86_64 zip 12,074, aarch64 msi 2,909, aarch64 zip 1,727. macOS v0.1.1 zip: 851. Active user count: **(unknown)** | `gh api repos/LGUG2Z/komorebi/releases/latest`, `.../komorebi-for-mac/releases/latest` |
| Distribution channels of Windows: Scoop Extras manifest (`checkver: github`, autoupdate URLs `.../releases/download/v$version/komorebi-$version-{x86_64,aarch64}-pc-windows-msvc.zip`, hash from `$baseurl/checksums.txt`), WinGet `LGUG2Z.komorebi` (workflow job `winget`, secret `WINGET_TOKEN`), MSI (wix), `cargo install --path komorebi-daemon --locked` from a clone (docs/installation.md:118), nixpkgs has `pkgs/by-name/ko/komorebi/package.nix`. Chocolatey: **not found** in the repo | `gh api repos/ScoopInstaller/Extras/contents/bucket/komorebi.json`; `.github/workflows/windows.yaml` job `winget`; `gh api repos/NixOS/nixpkgs/contents/pkgs/by-name/ko/komorebi/package.nix`; `grep -i choco` finds nothing |
| Secrets in the Windows repo: `PAT`, `SCOOP_TOKEN`, `WINGET_TOKEN` (names only); KDE and macOS (public) repos list none | `gh api repos/.../actions/secrets` |
| Windows CI wall time: 22 to 30 minutes per run (`master` push 2026-10-31 01:21:55 to 01:51:51; schedule 2026-11-01 00:51:43 to 01:14:06; feature-branch pushes 15 to 22 minutes) | `gh api repos/LGUG2Z/komorebi/actions/runs` timestamps |
| Users of the update checks: the Windows bar and `komorebic` read `https://api.github.com/repos/LGUG2Z/komorebi/releases/latest` (komorebi-bar/src/windows/os.rs:35; komorebic/src/windows.rs:585, 950); the macOS bar reads `.../komorebi-for-mac/releases/latest` (komorebi-bar/src/macos/os.rs:18, 22, 123) | `git grep` at `2185556efba6` |

Two premises of the brief could not be reproduced: "the 23 macOS commits that mention `#N`": in the macOS history only ONE commit
matches `#[0-9]` (`6b980a03 fix(border): close nswindow on main thread`, whose `#0`..`#17` are stack frames in a log excerpt,
not issue references: inferred from the body); the KDE history has none. The 309 commits matching `#[0-9]` in the
merged history (`git log 2185556efba6 --grep='#[0-9]'`) are overwhelmingly Windows commits, whose `(#1234)` references
are to this repository and stay correct. So nothing needs rewriting; a rewrite is excluded anyway (original hashes are a decision).

## 1. Branch and history mechanics

### 1.1 Decision and owner

Decision: how the consolidated line becomes `LGUG2Z/komorebi` `master`, and what is published first. Owner: the repository owner (the
user). Agents never push.

**One gate before any option: publishing the KDE history.** `komorebi-for-kde` is private (section 0). Pushing `wip-main`
publishes its 254 commits and the KDE source to a public repository. That cannot be taken back (forks, clones, GitHub's
commit cache by hash, search engines). The Windows history already public (85 commits on `komomono`) contains no KDE
code (it is the pre-merge tree: `git -C ~/komorebi log` of `komomono` has no `komorebi-for-kde/` path; **inferred** from the merge order in the
progress log: A2 merged KDE and macOS on top of `komomono@source`). The macOS history is mostly public already (`ash-squashem` is a public branch of `komorebi-for-mac`, `gh api .../branches`), but the private
repository `KomoCorp/komorebi-for-mac` held the development (docs/macos/README.md:46-47); the 41 commits that are only on `ash-squashem`
are public today. The owner has decided that there is no private repository afterwards. All 254 + 304 authors are one address (`LGUG2Z@fastmail.com`; `git log --format=%ae`, plus one dependabot) so no third-party privacy question arises
from authorship. A licence question remains: the tree carries `LICENSE.md` "Komorebi License 2.0.0" and the Homebrew template states the same licence
(docs/dev/old-ci/macos/formula.template.rb:8). The owner decided that KDE and macOS become public with the Windows project and that there is no private mirror (R1,
answered). **The push itself still needs the owner's explicit confirmation at step L3 (section 5): do not push before it.**

### 1.2 Options

| Option | What it does | For | Against |
|---|---|---|---|
| **A. Fast-forward `master` to the cutover tip (recommended)** | `master` (`72aa4c97`) is an ancestor of the local line, so `git push origin <tip>:refs/heads/master` is a fast-forward, no force. `komomono` stays as a pointer for a while | keeps `LGUG2Z/komorebi` (stars, forks, issues, PR numbers, `releases/latest`, Scoop/WinGet/nixpkgs URLs, `cargo install --git`, the update checks) and all original hashes; no history rewrite | master changes shape in one step (see 1.4); needs a master freeze while it is prepared |
| B. Make `komomono` the default branch, leave `master` | change default branch in settings | master untouched, reversible by one setting | `cargo install --git` and every clone/`git clone` default follow the default branch, so they change anyway; 27 open PRs target `master` and would have to be retargeted; docs say `master` (`mkdocs.yml`, `blob/master`, `raw.githubusercontent.com/LGUG2Z/komorebi/master/schema.asc.json`); two long-lived trunks |
| C. Rename branch `komomono` to `master` through GitHub's rename | server-side rename moves PRs, protection, Pages | none over A | `master` already exists; renaming over it is what A does without risk. Not needed |
| D. Squash or rebase the line onto `master` | one new commit set | simple log | rewrites every hash; breaks the binding "original commit hashes kept" decision; needs a force-push. Rejected |
| E. A new repository (`LGUG2Z/komorebi-all` or an organisation) | clean start | no effect on Windows users until chosen | loses stars (15,331), forks, 755 issues, PR history, `releases/latest` of every installed updater; GitHub offers no way to move them. Rejected |

Recommendation: **A**, in two pushes: (1) push the local line to the branch `komomono` (a fast-forward: `komomono@origin` is its ancestor); review in
the browser on GitHub (compare `master...komomono`); (2) after the go/no-go checklist, push the same commit to `master`. Both pushes are
fast-forwards. Never use `--force` or `+refspec`: if a push is rejected, something moved, stop and read why.

### 1.3 What happens to the 85 commits already on `komomono@origin`

They are the first 85 of the 1151 commits of `komomono@source..wip-main` (at `wip-main` = `6067938e`, which is the 41 revisions of the cutover stack, the one flake and the checks on `21cf0d73`). The local line is a descendant (`jj log -r 'wip-main..komomono@source'` empty), so pushing `wip-main` onto
`komomono` adds 1151 commits and rewrites nothing. The cutover stack (renames, version, one flake) is NOT on origin yet (they exist only in
the work clone); the work clone's history of `wip-cand*` bookmarks is local scaffolding and must NOT be pushed (see 1.6).

### 1.4 Effect on open PRs, issues, forks and `master` users

- **Open PRs (27: 19 community, 8 dependabot).** Their merge base is old `master`, so GitHub's diff still shows only their changes. Merging them after the
  cutover is a three-way merge across renames. The Windows daemon moved from `komorebi/` to `komorebi-windows/`, the root `schema*.json` to `schemas/windows/`, the
  `komorebi` package directory is now `komorebi-daemon/` (its package name is still `komorebi`; `komorebi-daemon/Cargo.toml`). `git merge` uses rename detection, so
  many will merge cleanly **(inferred, untested)**; each contributor may be asked to rebase. Whether they merge cleanly: **(unknown)**, test on the scratch repo (section 5.4) with
  `git merge-tree` for each head in `gh pr list`.
  Plan (master is already frozen): merge or close with a comment those that are ready; for the rest post one comment with the path map (`komorebi/` to `komorebi-windows/`, `schema*.json` to
  `schemas/windows/`, `komorebi/src/main.rs` to `komorebi-daemon/src/main.rs`) and keep them open. The 8 dependabot PRs: close them; dependabot recreates them against the new `dependabot.yml` (section 3.7).
- **Contributor path map (final layout).** Libraries and dev tools live under `crates/`, shipped binaries stay at the top level. A patch written against old `master` or against an earlier state of the cutover stack is rewritten with:

  | old path | new path |
  |---|---|
  | `komorebi/` (the OLD Windows daemon, e.g. `komorebi/src/window_manager.rs`) | `crates/komorebi-windows/` (NOT the new `komorebi/`, which is the thin binary package: a PR that touches `komorebi/src/*` of old `master` must be rewritten by hand against `crates/komorebi-windows/src/*`) |
  | `komorebi-daemon/` (cutover stack, before the final layout) | `komorebi/` |
  | `komorebi-{windows,linux,macos,hotkeys,client,layouts,scrolling,themes}/` | `crates/komorebi-{windows,linux,macos,hotkeys,client,layouts,scrolling,themes}/` |
  | `e2e/guest/agent`, `e2e/guest/testwin` | `crates/komorebi-e2e-agent`, `crates/komorebi-testwin` |
  | `komorebic/fixtures/` | `crates/komorebic-core/fixtures/` |
  | `schema*.json` (root) | `schemas/windows/` |

  The rewrite recipe for a patch series (the Windows daemon rule must come first, so that its output is not caught by the `komorebi/` of the last rule):
  `git format-patch -o /tmp/p origin/master..mybranch && sed -i -E 's#([ab]/)komorebi/#\1crates/komorebi-windows/#g; s#([ab]/)komorebi-(linux|macos|hotkeys|client|layouts|scrolling|themes)/#\1crates/komorebi-\2/#g; s#([ab]/)komorebi-daemon/#\1komorebi/#g; s#([ab]/)e2e/guest/agent#\1crates/komorebi-e2e-agent#g; s#([ab]/)e2e/guest/testwin#\1crates/komorebi-testwin#g; s#([ab]/)komorebic/fixtures/#\1crates/komorebic-core/fixtures/#g' /tmp/p/*.patch && git checkout -b mybranch-rebased <new master> && git am -3 /tmp/p/*.patch`
  (`git am -3` also falls back to rename detection for what the sed misses; the `index` lines of the patches need not be changed).
- **Open issues (89).** They stay: same repository, same numbers. Add labels `windows`, `linux`, `macos` and a platform field in `.github/ISSUE_TEMPLATE/bug_report.yml` (Windows-only today).
- **Forks (376).** Unaffected until they sync. A fork's `master` fast-forwards in GitHub's "Sync fork" (no divergence unless the fork's master has commits, in which case they face the rename conflicts).
- **`master` users.** Anything tracking `master`: `cargo install --git https://github.com/LGUG2Z/komorebi komorebi` still resolves the package `komorebi`
  (`komorebi-daemon/Cargo.toml` name = "komorebi"), but it is now the cross-platform package, and on Windows needs the `komorebi-windows` crate, which builds (checked on the cutover stack: "Windows clippy" per the progress log).
  Consumers of `komorebi-client = { git = ... }` keep the package name `komorebi-client`. Raw schema URLs `.../LGUG2Z/komorebi/master/schema.asc.json` (komorebic/applications.json:2, written into users' home by
  `komorebic quickstart`, komorebic/src/windows.rs:354-363) and `.../master/schema.json` (spikes fixture) point at root files that **no longer exist**
  (`schemas/windows/` now). Editors lose validation for those users; nothing crashes (see 2.2b).
  Because the root schema files are gone (decision R5), these raw `master/` URLs stop resolving; the consequence, the tasks and the wording for the README and the release notes are in 2.2b. Tagged URLs (`.../v0.1.41/schema.json`, used by `docs/komorebi.example.json:2`) stay valid forever (tags are immutable).
  The Windows tests fetch `.../refs/tags/v{version}/komorebi.example.json` (komorebi-windows/src/static_config.rs:1389) which also remain valid.
- **winget / Scoop.** Scoop and WinGet read releases, not branches (`checkver: github`). Their asset-name patterns must be kept exactly:
  `komorebi-<version>-<x86_64|aarch64>-pc-windows-msvc.zip`, `komorebi-<version>-<x86_64|aarch64>.msi` and `checksums.txt` (Scoop reads `$baseurl/checksums.txt`; adding macOS and Linux lines to
  the same file should be safe because the hash is looked up by file name, **inferred**, verify in the dry run). The Windows `latest` release becomes the unified release, so the pattern holds only if every unified
  release keeps the Windows assets. There is no upstream nixpkgs package for komorebi to update (owner: only this flake); the flake of this repository is the Nix distribution.
  Chocolatey: not present in the repository or workflows; **(unknown)** whether a third-party package exists.

### 1.5 Protection rules

Today there are none (`branches/master/protection` 404, rulesets 0). After the cutover add a **ruleset** on `master` and on tags `v*`: block force-push and deletion, require a pull request for everyone except the owner (bypass for owner), require the
one required check named `ci / required` (section 3). The KDE repo could not have protection at all ("Upgrade to GitHub Pro or make this repository public": `gh api .../branches/master/protection`), which is one more small reason to move it. The ruleset is not installed before the push because a
protected `master` that rejects the first fast-forward from the owner would need a bypass anyway.

### 1.6 What not to push

The work clone `/home/jado/komomono-work/komorebi` has about 60 `wip-*` bookmarks (`jj bookmark list -a`), including `wip-cand1`..`wip-cand9`, `wip-u-*`, `ws-*`, and the `kde`/`mac` remotes pointing into the scratchpad. Push exactly **one** commit id, never `--all` and never `jj git push --tracked`. In `~/komorebi`: import the stack as one bookmark (commands in 5.3).

### 1.7 The old-hashes check (run before and after the push)

Evidence already seen on `2185556efba6`:
- `git log --follow` and `git blame` follow the pure moves: `git log --follow -- komorebi-windows/src/splash.rs` lists 7 commits back to `b6139864 2025-10-28 feat(wm): add a splash screen for mdm devices` (ran);
  `git blame -L1,2 -- komorebi-windows/src/splash.rs` names `b61398647 komorebi/src/splash.rs (LGUG2Z 2025-10-28)` (ran). The same `--follow` for `komorebi-macos/src/splash.rs` reaches the same original commit (7 commits; the macOS repository forked from the Windows history).
- `jj file annotate -r wip-cutover-docs komorebi-windows/src/splash.rs` does **not** follow the move: every line is attributed to the move revision (`ywyyvwzz`, 2026-11-01 07:23). Use `git blame`/`git log --follow` (or `jj file annotate` at the revision before the move) for authorship. The unified-binaries note already says "jj does not follow moves" (docs/dev/unified-binaries.md).
- `git log -- <new path>` without `--follow` shows 1 commit (the move): expected.

Checks to run on the pushed state (all read-only; expected output in brackets):
```sh
for c in 72aa4c97ade72b802a008bdba4bf59cca04df665 7d19fd404534d75edfcece09831143c36896d40b 9a58e5117afa2df6bfbe544a1c40f2534935f094 ff3711dcb5d2a1cbef5e3e91523c0183bc50343e; do
  git merge-base --is-ancestor $c origin/master && echo "ok $c"; done          # four "ok"
git merge-base --is-ancestor origin/master~0 origin/master                       # trivial
git rev-list --count 72aa4c97ade72b802a008bdba4bf59cca04df665..origin/master    # equals the local count (about 1195 + the revisions added after this plan)
for t in v0.1.0 v0.1.1 v0.1.41; do git rev-parse $t^{commit}; done              # unchanged Windows tags (v0.1.0 and v0.1.1 are Windows commits, not macOS)
git rev-parse 64341e29a2d55b0b2f4289687944eec5aa519136 0cc4ca7f67d79da0d59d603dde2f6b4445fec85b   # the macOS v0.1.0 and v0.1.1 commits still resolve
git log --follow --format=%h -- komorebi-macos/src/splash.rs | tail -1          # b6139864
```
In the old repos, count what will be pointed at: `git -C ~/komorebi-for-kde rev-list --count master` (254) and `git -C ~/komorebi-for-mac rev-list --count ash-squashem` (304).

### 1.8 Steps (summary; commands in section 5.3)

1. `master` is already frozen by the owner; record its sha (`72aa4c97` at the time of writing). Merge the ready PRs, comment the path map on the rest, close the 8 dependabot PRs. 2. Rehearse on a scratch repository (5.4). 3. Fetch the final stack into `~/komorebi`, verify. 4. Push to `komomono` (fast-forward). 5. After sign-off, push the same commit to `master`. 6. Safety branch `legacy/windows-0.1` at `72aa4c97` (so
`cargo install --git ... --branch legacy/windows-0.1` and hotfixes for 0.1.41 remain possible). 7. Ruleset. 8. Keep `komomono` for two weeks, then delete the branch (reversible while commits are reachable from `master`).

### 1.9 Verify, roll back, irreversible

Verify: the checks in 1.7; `gh api repos/LGUG2Z/komorebi/branches/master --jq .commit.sha` equals the pushed id; GitHub's compare of `72aa4c97...master` shows "ahead by N, behind by 0".
Roll back **before** the push to `master`: nothing happened (delete the scratch repo, forget the work). Roll back **after**: there is no safe rollback except a *forward* revert (a commit on `master` that restores the old tree) or, as a last resort, a force-push of `master` back to `72aa4c97` which the Windows users' clones would see as a rewritten history and which fights with every PR merged in between: do not do it; instead keep `legacy/windows-0.1`.
**Irreversible:** publishing KDE's source and history (commits become public and mirrored by forks/caches); anything a third party cloned; the tag `v0.2.0` once Scoop, WinGet or nixpkgs bots picked it up (section 4).

## 2. The two old repositories

### 2.1 Decision and owner

Decision: what each old repository becomes after the cutover. Owner: the user. **KDE: archive after the cutover (no redirect needed, it is private; nothing to keep alive).** **The old macOS repository (`LGUG2Z/komorebi-for-mac`) is handled by the owner (R7): this plan has no step, banner, mirror release or archive date for it.** (Facts kept for the owner: 851 downloads of macOS v0.1.1; the already-installed 0.1.x macOS bars read `komorebi-for-mac/releases/latest`, komorebi-bar/src/macos/os.rs:18.)

### 2.2 Issues and PRs

| Repo | Open issues | Open PRs | Method |
|---|---|---|---|
| `komorebi-for-kde` | 1 (#6, the canary issue opened by the CI) | 0 (5 total, all closed) | close #6 with a pointer; nothing to transfer |
| `komorebi-for-mac` | **issues are disabled** (`gh issue list` says so); `open_issues_count` shows 13 but is stale or counts something the API will not list: **(unknown)**; check the web UI | 0 (0 ever) | nothing to transfer; if the 13 are really issues, enable issues for the transfer window and use `gh issue transfer N LGUG2Z/komorebi` (GitHub moves issues between repositories of one owner: all three are `LGUG2Z`) |
| `komorebi` | 89 | 27 | stays; the unified tracker |

Method recommendation: one tracker (`LGUG2Z/komorebi`), label by platform; do not transfer anything automatically. A transfer keeps the issue number-to-title mapping only through a redirect from the old URL, and the numbers change, so for 0 to 1 issues it is not worth it.
The `[FEAT]:` sponsor-only auto-close policy (`.github/workflows/feature-check.yaml`) now applies to Linux and macOS feature requests too (decided, R6); the KDE repo had no such policy. What must be rewritten for it to make sense is the task list 2.2a.

#### 2.2a R6 task list: the sponsor-only `[FEAT]` policy for one public project with no private tier

**Status: done.** The owner said "you can update these"; the agent `sponsor-texts` rewrote the texts (its revisions are on the dev line); check the list below against them when reviewing.

Found by `git grep -i sponsor` on the tip (`59b01fdb`, plus the checks of `2185556efba6`):
1. `.github/workflows/feature-check.yaml:22` (message text): names "current GitHub sponsors on the $5/month tier and above", "valid individual commercial use licence" and "approved contributors". Rewrite so it does not imply a private repository or early access, says it is for all platforms (Windows, Linux, macOS), and links the one licence page. Keep the `[FEAT]: ` title prefix check and the close-and-lock behaviour.
2. `.github/ISSUE_TEMPLATE/feature_request.yml:2,11,13,16,17,30` (description "Limited to Sponsors, Commercial License Holders, and Collaborators", the sponsorship platform dropdown with GitHub Sponsor and Ko-fi, the username field): add a required **platform** dropdown (Windows, Linux KDE, macOS); keep or adjust the sponsor fields as the owner words them.
3. `.github/ISSUE_TEMPLATE/bug_report.yml` and `config.yml`: add the platform field and fix the docs link (they point at `lgug2z.github.io/komorebi/...`, Windows-only) so Linux and macOS reporters are not sent to the wrong docs.
4. `README.md:16-17,102,134-152` and `docs/index.md:48-65`: the "Sponsorship for Personal Use" section and the sentence about "submit feature requests on the issue tracker" are written for the Windows project; reword for the three platforms.
5. `docs/linux/index.md:124-141` and `docs/macos/index.md:59` and `docs/macos/README.md:19-20,43-47,60`: the same section is copied into the Linux and macOS indexes; remove the paragraph that says the public repository is for releases and a private one for certain sponsor tiers (`docs/macos/README.md:43-47`), keep or delete the copies of the sponsorship text so that there is ONE wording; check the badge lines.
6. `docs/dev/old-ci/macos/formula.template.rb` (caveat about a private repository and token) is not copied into the new formula (see 2.4).
7. Licence: `LICENSE.md` (Komorebi License 2.0.0) is the one licence; the macOS formula states `license "Komorebi License 2.0.0"` (formula.template.rb:8) and the KDE tree had its own file merged into one at the cutover: confirm that no per-platform licence text remains (`git ls-files | grep -i licen`) and that `komorebic`'s licence prompt text (`komorebic/src/windows.rs` and `komorebic/src/macos/mod.rs`, which name `https://lgug2z.com/software/komorebi`) is the same on all platforms.
8. Decide what the policy does for PRs and for the KDE feature set that was developed privately (nothing to do in code; say it in the README).

#### 2.2b R5 consequence: the old Windows `master/` schema URLs stop resolving

No root `schema.json`, `schema.asc.json` or `schema.bar.json` remains (checked: none at the root of the tip). Therefore:
- `https://raw.githubusercontent.com/LGUG2Z/komorebi/master/schema.asc.json` (the `$schema` line of `komorebic/applications.json`, copied by `komorebic quickstart` into users' homes) and `.../master/schema.json` return 404 after the `master` push. Nothing crashes: only editor validation and completion for those users stops. Users who copied configs with `.../v0.1.41/schema.json` or another tag keep working (tags are immutable); the `komorebic quickstart` of the OLD 0.1.x binaries writes the `master/` URL, so every user who ran it has one.
- Repository tasks: change `$schema` in `komorebic/applications.json` and in `komorebic/src/macos/applications.json` to the per-platform path (`schemas/<platform>/schema.asc.json`) at the tag chosen under R5; `docs/komorebi.example.json:2` and `docs/komorebi.bar.example.json:2` (they say `v0.1.41`) to the new tag at the release; the spike fixture `spikes/config-parity/fixtures/windows-kitchen-sink.json` can stay.
- During the dev line (`master` has the new tree but no `v0.2.0` tag), a tagged URL for the new paths does not exist yet; use `.../master/schemas/windows/schema.json` in the dev-line docs only, and switch to the tag at E1.
- README and release notes must say, in these words or close: "The Windows schema files moved from the repository root to `schemas/windows/`. A `$schema` line that points at `.../LGUG2Z/komorebi/master/schema.json`, `schema.bar.json` or `schema.asc.json` no longer resolves; change it to `.../LGUG2Z/komorebi/v0.2.0/schemas/windows/schema.json` (and `schema.bar.json`, `schema.asc.json`). Lines that name a release tag keep working." Put the sentence in `README.md` (a "Upgrading from 0.1.x" note) at the dev-line landing, and in `RELEASE-NOTES-0.2.0.md` (4.5) as the first migration item for Windows users. Also announce it where the user community reads (the README names Discord).

### 2.3 Where the old names are used (classified)

Counted by `git grep` at `2185556efba6` (excluding `docs/dev/old-ci`): `komorebi-for-kde` 24 files / 98 lines; `komorebi-for-mac` 35 files / 124 lines; `lgug2z.com` 24 files / 37 lines;
`lgug2z.github.io` 5 files / 10 lines; `KomoCorp` 2 files / 11 lines; `homebrew-tap` 1 file / 3 lines.

| Class | Where | Action at cutover |
|---|---|---|
| **A. Runs in a shipped binary (must change before 0.2.0, otherwise a behaviour is wrong)** | `komorebi-bar/src/macos/os.rs:18,22` (update check and release link to `LGUG2Z/komorebi-for-mac`), `:123` (test strings, `v0.1.2`) | point at `LGUG2Z/komorebi/releases/latest`; but note the Windows bar already reads the same URL: after unification the macOS and Windows bars would both see `v0.2.0` and each other's tags. The release tag must therefore stay one number for all (it does), and the "is there a newer release" comparison needs no platform filter. The test at `:123` must follow |
| A. Schema URLs generated into shipped schemas | `crates/komorebi-config/src/changes.rs:248`, `crates/komorebi-bar-core/src/schema/changes.rs:29` (text becomes `schemas/macos/schema*.json` descriptions: "`$schema`: https://komorebi-for-mac.lgug2z.com/komorebi.0.2.0.schema.json") | decide the URL scheme (R5), change the text, regenerate `schemas/macos/` with `just jsonschema` on a Mac and the `nix-options` check |
| A. Application-specific config | `komorebic/src/macos/applications.json:2` (`$schema` `.../komorebi-for-mac/master/schema.asc.json`) | new path `schemas/macos/schema.asc.json` of the unified repo, at a tag or `master` |
| B. Example configs, copied into users' homes | `docs/macos/komorebi.example.json`, `docs/macos/komorebi.bar.example.json` (`$schema` `https://komorebi-for-mac.lgug2z.com/komorebi*.0.1.0.schema.json`), `docs/komorebi.example.json:2` (`.../komorebi/v0.1.41/schema.json`) | the 0.1.x URLs stay valid only while the hosts exist: **keep `komorebi-for-mac.lgug2z.com` serving the 0.1.x schemas** (do not delete the Cloudflare Pages project), and add the 0.2.0 schemas under the chosen scheme |
| C. Install instructions | `docs/linux/installation.md:27,167,181-182` (`github:LGUG2Z/komorebi-for-kde`, `git clone .../komorebi-for-kde`), `docs/macos/installation.md:48-51,80` (`brew install lgug2z/tap/komorebi-for-mac-nightly`, `inputs.komorebi-for-mac.url = "github:KomoCorp/komorebi-for-mac"`), `docs/macos/README.md:43-51,272` | rewrite to `github:LGUG2Z/komorebi`; the flake attribute names wait for `flake-one` (see `docs/dev/flake.md`: `.#checkSets.<set>`, `.#komorebi`); note `nixosModules.komorebi-for-kde`, `overlays.komorebi-for-kde`, `darwinModules.komorebi-for-mac` are in today's root flake (flake.nix:407-409): keep these names as aliases of the new ones for one release |
| C. READMEs and prose | `README.md:72` ("The Mac port used to live in its own repository ... where this repository will be published is not decided"), `docs/linux/README.md:5,81`, `docs/linux/dev/*.md`, `docs/macos/README.md` badges (commented out) | edit once the repository is final; R1 and R3 decide the wording |
| D. History, left alone on purpose | `docs/dev/*.md` analysis notes, `docs/linux/CONTRIBUTING.md:107-108`, `docs/linux/dev/COMPARE.md`, `scripts/linux/compare-gen.py` (stale, flagged in MIGRATION-SUMMARY.md section 6), `crates/komorebi-version/src/lib.rs:15` (a comment link), `tools/fast-check`, `tools/kde-closure-check`, `tools/e2e-needed` (old directory names; `tools/kde-closure-check` still reads the old path of `root-crates.json`, MIGRATION-SUMMARY.md section 7) | the tools lines are code: fix them in the pre-cutover stack (they affect `tools/fast-check`'s routing, which will mis-route everything now); the prose is history |
| E. Release machinery | `docs/dev/old-ci/macos/workflows/*.yaml` (`KomoCorp/komorebi-for-mac`, `LGUG2Z/komorebi-for-mac`, gist `78b3dd12a7b7069f1d1c60621fe50730`, `LGUG2Z/homebrew-tap`), `formula.template.rb` (`homepage "https://github.com/KomoCorp/komorebi-for-mac"`, token header for a private asset URL) | rewritten in section 3 and 4, not copied |
| F. Sites and links on lgug2z.com | `https://lgug2z.com/software/komorebi` (licence purchase, in `README.md`, `komorebic/src/windows.rs` x3, `komorebic/src/macos/mod.rs`, `feature-check.yaml`), `lgug2z.com/articles/educational-source-software/`, `kw-icul.lgug2z.com` and `km-icul.lgug2z.com` (splash.rs of Windows and macOS), `komorebi.lgug2z.com/schema` and `komorebi-bar.lgug2z.com/schema` (mkdocs.yml, docs/usage/*.md, docs/common-workflows/bar.md), `komorebi-starlight.lgug2z.workers.dev` (the reference sites, README.md, docs/macos/README.md) | outside this repository. Which host serves which site and whether DNS stays: **(unknown)** (not checked; a `curl -I https://komorebi.lgug2z.com/komorebi.0.1.41.schema.json` returned 404, so the Windows schemas are not published under that name today) |

### 2.4 Crate, package and tap names

- Cargo packages that disappeared (deleted shim packages): `komorebic-linux`, `komorebic-macos`, `komorebi-client-linux`, `komorebi-client-macos`, `komorebi-bar-macos` (MIGRATION-SUMMARY.md section 7). Directories `komorebi/` (Windows) is `komorebi-windows/`. None of these was published to crates.io as far as the repository says (no `publish` key, no `cargo publish` in any workflow); whether any name was ever published: **(unknown)**, check `cargo search komorebi` before 0.2.0.
- Homebrew tap `LGUG2Z/homebrew-tap`: `Formula/komorebi-for-mac.rb` and `Formula/komorebi-for-mac-nightly.rb` exist (`gh api repos/LGUG2Z/homebrew-tap/contents/Formula`). The template downloads from the private `KomoCorp` release with a token header (docs/dev/old-ci/macos/formula.template.rb:4-9). For the public unified repo the formula can use a plain public URL. Recommendation: keep the formula **name** `komorebi-for-mac` for 0.2.0 (users' `brew upgrade` keeps working), point its `url` at the unified release asset `komorebi-0.2.0-aarch64-apple-darwin.zip`; rename later with a tap rename file **(inferred, Homebrew taps support `formula_renames.json`)**.
- Flake inputs of downstream users: `github:LGUG2Z/komorebi-for-kde` and `github:KomoCorp/komorebi-for-mac`. After archiving, an archived repository still serves flakes (archive is read-only, not removed), so users do not break; they stay on the frozen revision. Their inputs must be changed by hand to `github:LGUG2Z/komorebi` (a tag `v0.2.0` or `master`). Say it in the release notes. **Do not delete or rename** either repository (a rename keeps redirects for git and web but not for flake `github:` inputs reliably: **inferred**; deleting breaks all of it).

### 2.5 Steps

1. **KDE** (owner, by hand, after the 0.2.0 release or when the owner says so): one commit on its `master` with a README banner "development moved to LGUG2Z/komorebi" and the new install commands (GitHub's web editor or a clone in a temp dir; never `~/komorebi*`), close issue #6 (the canary issue), then archive (Settings, Archive: reversible).
2. **macOS repository: the owner handles it himself (R7).** Nothing in this plan.
3. Keep the Cloudflare Pages project and DNS for `komorebi-for-mac.lgug2z.com` as they are (schema URLs of released 0.1.x configs).
4. The macOS origin token is NOT rotated (owner decision); rotate a PAT whose name appears in the old workflows only if exposure is suspected: `GH_USER_API_TOKEN`, `PAT`, `SCOOP_TOKEN`, `WINGET_TOKEN`.

### 2.6 Verify, roll back, irreversible

Verify: `gh repo view LGUG2Z/komorebi-for-kde --json isArchived`. Roll back: archive and README banner are reversible. **Irreversible:** deleting either old repository (macOS: stars 261, forks 4 vanish), a rename (breaks flake inputs), transferring ownership away (not proposed).

## 3. CI

### 3.1 Decision and owner

Decision: the one workflow set, runners, triggers, what is a nix check and what is cargo. Owner: the user (cost, self-hosted hardware, secrets). Agents may only draft. Nothing here is created under `.github/`; the drafts are in Appendix A.

### 3.2 What exists (read)

| Old workflow | Jobs and facts (file) |
|---|---|
| Windows `.github/workflows/windows.yaml` | `cargo-deny` (ubuntu-22.04); `build` matrix `windows-latest` x `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`: CRLF check, `cargo +nightly fmt --check`, `cargo clippy`, `cargo test`, `houseabsolute/actions-rust-cross` release build, `cargo wix --no-build -p komorebi ... -I .\wix\main.wxs`, upload artifact (14 days) with `Swatinem/rust-cache`; `nightly` (schedule `30 0 * * 0` = every Sunday, the comment says "every day": the comment is wrong, only on `master`; deletes and recreates the `nightly` prerelease, notes by `kokai`), `release-dry-run` (draft release), `release` (on `v*` tags, `softprops/action-gh-release@v2`), `winget` (`vedantmgoyal2009/winget-releaser@main`, `WINGET_TOKEN`). Triggers: PR, push to `master`, `feature/*`, `hotfix/*`, tags `v*`, schedule, dispatch. `permissions: write-all` |
| Windows `feature-check.yaml` | closes `[FEAT]: ` issues unless a condition stated in its message holds (see R6) |
| KDE `docs/dev/old-ci/linux/workflows/ci.yaml` | all jobs guarded by `github.repository == 'LGUG2Z/komorebi-for-kde'`; `cargo-deny`; `checks` (3 shards by regex: `rust`, `guests`, `rest`, all as `nix build` of `checks.x86_64-linux.*` except `vm-*`, `fail-fast: false`); `lint` (`nix develop --command just lint`); `discover` (reads `legacyPackages.x86_64-linux.vmTiers`); `vm-fast` (one runner per suite, KVM, 45 min timeout, `nix build .#checks.x86_64-linux.vm-<suite>`); `vm-slow` (master, schedule, dispatch or PR label `ci:vm-slow`; 60 min; runs the test driver outside the sandbox to upload screenshots and journals); `canary` (weekly: moves `nixpkgs` to the tip in a copy, builds every check, VM tiers included, one machine, 360 min timeout, opens or updates an issue); `build` (x86_64 on `ubuntu-24.04`, aarch64 on `ubuntu-24.04-arm` `continue-on-error`; patchelf to the system loader; runs `--version`); `publish` (tags `v*` and the `nightly` prerelease from master schedule/dispatch; zip per target, `schema.json`, `checksums.txt`, `kokai@0.3.1` notes) |
| KDE `actions/nix/action.yaml` | frees disk on KVM jobs (a Plasma guest is about 10 GB), enables KVM through a udev rule and fails with a named step "Enable KVM" when absent, `cachix/install-nix-action@v31`, **no Cachix service is used** ("no Cachix cache, account or token is configured"), cache `nix-community/cache-nix-action@v7` keyed on `flake.lock` and `**/Cargo.lock` with `gc-max-store-size-linux: 8G` |
| KDE `rerun-no-kvm.yaml` | reruns failed jobs up to 3 attempts when every failure is the "Enable KVM" step; runs on `workflow_run` of `ci`. Observed: 5 runs in the history |
| KDE `update-flake-lock.yaml` | Mondays 03:00 UTC, `DeterminateSystems/update-flake-lock@v29`, PR "chore(deps): flake.lock: Update" |
| macOS `docs/dev/old-ci/macos/workflows/nix.yaml` | `runs-on: self-hosted`, guarded to `KomoCorp/komorebi-for-mac`: `nix flake check`, `nix build`, upload `result/bin/komorebi*`, on master deploy `schema.json` to a gist (`exuanbo/actions-deploy-gist`, secret `GH_USER_API_TOKEN`), on master schedule/dispatch publish the `nightly` prerelease and push a generated formula to `LGUG2Z/homebrew-tap` |
| macOS `release.yaml` | manual dispatch with a tag; checks that the tag is at the same commit in both the private and public repositories; `nix flake check`, `nix build`, zip `komorebi-<tag>-aarch64-apple-darwin.zip` (the name includes the `v` of the tag; Windows and Linux names do not) and `checksums.txt`; creates the release in both repos |
| Three `dependabot.yml` | github-actions and cargo, weekly, assignee LGUG2Z, commit prefix `chore` with scope: the Windows one is live, the other two identical (docs/dev/old-ci/*/dependabot.yml) |

### 3.3 Proposed design

Principles: (1) **required check = one job, `required`, that depends on the others** so protection names one check; (2) everything that runs without a window system runs on the PR; (3) anything that needs KVM or a Mac or the evaluation Windows image does not run on fork PRs; (4) nix checks are the unit of truth for lint/clippy/nextest/doc/deny/fmt of each platform because the old repos already gate on them (the cutover docs list them, `docs/dev/build-and-test.md` sections 1-3); cargo jobs only where Windows or macOS must run natively (the MSI, `cargo test` on Windows).

| Job | Runner | Needs | Runs on | What |
|---|---|---|---|---|
| `deny` | `ubuntu-24.04` | | PR, push `master`, schedule | `EmbarkStudios/cargo-deny-action@v2` with the one root `deny.toml` (the cutover merged them) |
| `cheap` (matrix over the sets `fmt deny windows-lint windows-test linux-lint linux-test linux-options`) | `ubuntu-24.04` | | PR, push `master` | `nix build .#checkSets.<set>`: the cheap check set is the PR gate (`checkSets.cheap` = `defaultCheckSets` in `nix/checks.nix`; docs/dev/flake-checks.md). Windows clippy is the 3.5-minute pre-landing gate with warm dependencies (docs/dev/build-and-test.md section 1) |
| `windows-cross` | `ubuntu-24.04` | | PR, push | `nix build .#windows-komorebi .#windows-komorebic .#windows-komorebi-bar` (cross-built with xwin). The hosted `windows-latest` cargo job of the old workflow (fmt, clippy, `cargo test`, MSI) is NOT in the first CI (R9); the MSI moves to the release workflow (A.3). What that leaves untested is decision N1 |
| `macos` | `macos-14` (hosted arm64) | | PR, push | `nix build .#checkSets.macos-{lint,test,options}` and `.#komorebi .#komorebic .#komorebi-bar`: build, clippy and the darwin nix checks. `macos-nextest` runs 383 tests on the hosted macOS job (N1). **No daemon is started**: nothing in CI can verify tiling on macOS. A hosted macOS runner can compile and build: **(inferred, never tried here)** |
| `linux-build` | `ubuntu-24.04`, `ubuntu-24.04-arm` (optional) | | PR, push | the old KDE `build` job with `.#komorebi` and `.#komorebic`, patchelf to the system loader, `--version` |
| `vm-fast` (16 suites), `vm-slow` (18 suites) | **self-hosted with KVM (later phase)** | `discover` | nightly, manual | `legacyPackages.<system>.vm.<name>`; tiers from `legacyPackages.<system>.vmTiers`; the old design (one hosted runner per suite, 45/60 min timeouts, `rerun-no-kvm`) is the fallback if hosted KVM is used instead. KVM on hosted runners is documented for public repositories only (docs/dev/old-ci/linux/actions/nix/action.yaml:20-21); some hosts have none |
| `e2e-windows` | **self-hosted (later phase)**, label `kvm-windows-e2e` | | manual, nightly subset, weekly full, before a release | see below: needs KVM, the evaluation ISO and a locally built golden image (README:9-26); cannot run on a hosted runner **(inferred)** |
| `canary` | KVM runner, 360 min (later phase, with the self-hosted runner) | | weekly | as the KDE one: move nixpkgs, build every check against it, open or update an issue. Make it one job per platform group so it does not need 6 hours on one machine |
| `release` | see section 4 | `required` | tag `v*` | |
| `rerun-no-kvm` | `ubuntu-24.04` | | `workflow_run` | unchanged |
| `update-flake-lock` | `ubuntu-24.04` | | Monday 03:00 | `DeterminateSystems/update-flake-lock@v29`; the PR runs the full PR set |
| `feature-check` | `ubuntu-latest` | | issues opened | unchanged unless R6 says otherwise |

Triggers of the first CI: `pull_request` (all branches) and `push` to `master` only; **no `feature/*` or `hotfix/*` push triggers (owner decision R10: PR only)**; `workflow_dispatch`. Later or release phase: tags `v*` and the weekly `nightly` schedule (release.yaml), the nightly sweeps (sweeps.yaml, self-hosted). Note: the old schedule was weekly (`30 0 * * 0`), and the comments in two workflows say "every day": the nightly prerelease has been weekly in fact.

**Windows e2e subset (LATER PHASE, self-hosted; not in the first CI).** Owner decision R9: the owner will self-host later. Measured: 47 scenarios, 2.2 min typical (`commands` 6.2 min), first pass 19:54 to 21:45 = about 111 minutes, plus reruns and the soak (progress log "gate cand8"). The 22-scenario targeted gate took 00:33 to 01:25 (2.5-2.8 min each, `commands` 5.6; progress log "gate cand9"). Once a KVM-capable self-hosted runner exists (Appendix A.2): manual, label-gated run of the cluster named in a change plus `commands` (only when `tools/e2e-needed` says YES: it reads the Cargo closure of the Windows binaries); nightly the 22-scenario set of the cand9 gate (commands workspaces focus stacks monocle floating maximizing rules rules_class rules_exe manage_rules startup_rules title_rule layouts config_reload lifecycle handover resize lifecycle_hidden hotplug promoting), about 1 hour; weekly and before a release all 47, about 2 hours. It needs the Windows 11 Enterprise **Evaluation** ISO: the evaluation lasts 90 days, so the base image must be rebuilt about every 90 days (e2e/platforms/qemu_windows/README.md:20-24), or the nightly turns red on a date nobody chose. Never run fork code on that runner.
Known failures (docs/dev/build-and-test.md section 6; they do not block the dev-line landing, owner R4, and the release question is re-opened by the owner): four stale known-failure markers (`maximizing.user`, `placement`, `unknown_monitor` x2) fail on every commit, `fullscreen` fails 2 checks on baseline too (progress log 2026-11-01 00:30), a `commands` lock-hold stall about 1 in 10-12 sweeps. A green required status cannot be had from the full set until the four stale markers are re-marked and `fullscreen` fixed or marked: re-mark them first (R4).

**KDE VM tiers (later phase: nightly and manual on the self-hosted KVM runner, not a PR gate).** `fast` (16 suites: check hotplug commands kwin-restart kwin-events scrolling workspace-rules core-e2e kwin-contract kwin workspace-switch stacks module placement user-maximize run-kde) on PRs from the repository and on `master`; `slow` (18 suites) nightly and by label. One hosted runner each, in parallel, each up to the old 45/60 min timeouts. Measured on the user's host, not hosted: `commands` 1347 s at load about 2; the whole 16-suite cand9 run took about 65 minutes sequentially (00:02 to 01:07, progress log). Hosted wall time per suite: **(unknown)**; the old repo's `ci` runs on `master` failed within 7 to 10 seconds on push (2026-10-31 01:58 and 01:41) and about 10 minutes on the scheduled run (2026-11-01 00:50 to 01:01); the cause is **(unknown)** (not read: the job logs need `gh run view --log`, not run).

**Caching (decided, R12).** `actions/cache` (through `nix-community/cache-nix-action`, as the old KDE action did) with per-platform key prefixes (`nix-linux-<arch>-`, `nix-macos-<arch>-`, keyed on `flake.lock` and `**/Cargo.lock`, `gc-max-store-size-linux: 8G`), and `Swatinem/rust-cache@v2` for any cargo job. No Cachix for now. No Cachix was ever used by the old repositories (action.yaml comment). One repository now carries a Linux and a Darwin closure and the GitHub per-repository cache quota is small **(inferred: 10 GB for GitHub Actions caches; check current limits)**: the per-platform prefixes keep one platform from evicting the other; measure the hit rate on the first runs and revisit only if PR time exceeds 30 minutes.

**N1. The nextest decision: ANSWERED (2026-11-01).** The owner asked: can nextest run on a Mac with nix-darwin, and can the Windows unit tests run in a Windows VM from NixOS; if so, keep them. (1) **macOS: yes.** `macos-nextest` now has `doCheck = true` (`nix/macos/flake-module.nix`); built on the Mac Mini (aarch64-darwin, nix, no daemon): exit 0 in about a minute, 383 tests run, 383 passed, 1 skipped (the komorebi-macos library; the other packages' tests ran earlier through cargo, 753 in all). It needs a darwin builder, so the first CI runs it on the hosted macOS job (`checkSets.macos-test`). (2) **Windows: yes, in the Windows guest of the e2e harness, not as a pure nix check** (a Windows image cannot be in the store). The check `windows-nextest-build` (set `windows-test`) cross-compiles the 48 Windows test executables with xwin on a Linux builder (about 90 s warm); the app `windows-nextest` (`nix run .#windows-nextest -- OUT`, with `E2E_GOLDEN`, `KOMOREBI_E2E_SCALE`, `KOMOREBI_E2E_STATE` as for e2e-windows) uploads them with their data files to the guest and runs every test in its own process: 964 tests in 37 executables, 964 passed, 0 failed, 202 s with a restored guest state (one run, not soaked). It needs the same self-hosted KVM machine as the Windows e2e (later phase, R9). Also: the neutral crates (`komorebi-layouts`, `-scrolling`, `-themes`, `komorebic-core` and the shared `crates/*`) now run in `linux-nextest` (1532 tests, all pass), so they no longer depend on the Windows CI. What remains untested in the FIRST CI: the Windows-only crates' tests (they run in the self-hosted phase or in a hosted `windows-latest` `cargo test` job, the old fallback) and the macOS-only code at runtime.

**Cost and time.** Public repositories do not bill hosted Linux and Windows minutes **(inferred, GitHub's public-repo policy; macOS hosted runners are also free for public repos at the time of writing, check)**. The private KDE repository did bill. I found no billing data and do not give a cost figure. Times that were read: Windows workflow 22-30 minutes; KDE `vm-fast` timeout 45 min per suite in parallel; the PR critical path is therefore about the slowest of: Windows build 22-30 min, a VM suite up to 45 min, nix checks (Windows clippy 3.5 min warm, cold is **(unknown)**; the KDE shard run out of resources at about six minutes before it was sharded, ci.yaml comment lines 41-43).

**Concurrency and permissions.** Add `concurrency: group: ci-${{ github.ref }}, cancel-in-progress: true` for PR runs, but **not** for `master` or tag runs. Replace `permissions: write-all` (windows.yaml build, nightly, release) by `contents: read` at workflow level and `contents: write` only on release jobs. Self-hosted runner: never run fork PR code (a fork can edit the workflow): gate the job with `github.event.pull_request.head.repo.full_name == github.repository`.

**Secrets needed and who holds them.** `WINGET_TOKEN` (exists), `SCOOP_TOKEN` and `PAT` (exist; what `PAT` and `SCOOP_TOKEN` are used for is **(unknown)**: no workflow in the repository reads them, they may be for the Scoop bucket or leftover), a token to push the Homebrew formula to `LGUG2Z/homebrew-tap` (the old macOS workflow used `GH_USER_API_TOKEN`: create a fine-grained PAT limited to that repository, contents write), a Cloudflare token only if schema publishing moves into CI (`just schemapub` uses `npx wrangler pages deploy`, run by hand today), a cache token if Cachix. No signing secret exists anywhere: the Windows MSI is unsigned (nothing in `windows.yaml` signs) and the macOS formula ad-hoc signs at install (`codesign --force -s -`, formula.template.rb:19-21). The user holds all of them.

### 3.4 Dependabot

One `.github/dependabot.yml` with ecosystems `cargo` (`/`), `github-actions` (`/`) and `nix` (`/`, flake.lock; supported ecosystem name `nix` **(inferred)**: the existing workflow `update-flake-lock` already does this weekly, so choose ONE: keep `update-flake-lock` and do not enable `nix` in dependabot), `weekly`, `assignees: LGUG2Z`, `commit-message: prefix chore, include scope`. Add `groups:` for cargo (one PR for patch/minor) because one lock now serves three platforms and 900 packages (docs/dev/unified-binaries.md: "859 packages (root) -> 900") and eight separate PRs per week would be noise. The `pip` entry that GitHub auto-created in the KDE repo ("Graph Update: pip in /nix/tests/vm/lib") is the dependency graph for the VM library's Python, not a config entry.

### 3.5 Steps

1. Draft files in a revision on top of the cutover stack (user reviews): `.github/workflows/ci.yaml`, `e2e.yaml`, `release.yaml`, `canary.yaml`, `rerun-no-kvm.yaml`, `update-flake-lock.yaml`, and `.github/dependabot.yml` (Appendix A). 2. Dry-run them on the scratch repo (5.4). 3. Land them together with the `master` push; the first run on `master` is the only one that runs for the first time in public.
Constraint from the decisions: no KDE or macOS workflow runs before the cutover. Satisfied because workflows only trigger on `master`, PRs and tags and `komomono` is none of these (a PR from `komomono` to `master` WOULD run them: do not open one; push `master` directly).

### 3.6 Verify, roll back, irreversible

Verify: every job of the first CI green on the scratch repo with hosted Linux and hosted macOS (the self-hosted phase is verified later, when that runner exists). Roll back: delete or disable workflows (Actions tab) or revert the commit; runs and caches vanish with retention. Irreversible: workflow runs and Actions logs in the public repository; consumed secrets if mis-scoped (rotate).

## 4. The 0.2.0-dev landing and the release of 0.2.0

### 4.1 Decision and owner

Two phases (owner decision 3). **Dev-line landing:** `komomono` to `master` with the workspace at `0.2.0-dev`; no tag, no GitHub release, no
Scoop/WinGet/Homebrew/schema/docs publication, no announcement; CI on (it builds artefacts as workflow artifacts only, and the
weekly `nightly` prerelease is NOT enabled until the release phase, because `nightly` is a published release that Scoop-like consumers
and the update checks could see: disable the `nightly` publish step, or keep it to the dev line only once the owner says so). Consequence for users:
`releases/latest` stays `v0.1.41`, so the Windows update check, Scoop, and WinGet see no change; only people who build or install
from `master` see the new tree (1.4). **Release:** a revision on `master` that sets the version to `0.2.0`, then the tag, artefacts and announcement
(4.2 to 4.7). Decision: tag, artefacts, notes, channels. Owner: the user (secrets, announcements).

### 4.2 Version and tag policy

- One version: `[workspace.package] version` was `"0.2.0"` (Cargo.toml:61 at `2185556efba6`) and is `0.2.0-dev` on the line to land (as told by the coordinator, not re-read); the schemas say `v0.2.0` (commit `b8bbad87`): check that schema description text and `Cargo.lock` are consistent with `0.2.0-dev`, and regenerate both in the release revision (E1). `komorebi-version` is one crate. During the dev line **no `v0.2.0*` tag exists** (a `v0.2.0-dev` tag is not proposed either: a `v*` tag triggers the release workflow and Scoop/WinGet bots). **Tag `v0.2.0`** (release phase only) annotated, on the commit of `master` that passed the gates, pushed alone: `git tag -a v0.2.0 -m "komorebi 0.2.0" <sha>`; `git push origin v0.2.0`.
- Why v0.2.0 is monotonic for everyone: Windows is at 0.1.41, macOS at 0.1.1 (the bar even has a test string for `v0.1.2`), KDE never released. Semver orders 0.2.0 above all.
- Old tags: leave Windows `v0.1.*` as they are. The macOS tags `v0.1.0`, `v0.1.1`, `v0.1.0-prerelease` collide in name with Windows ones and were not imported; if the commits should be findable by name in the unified repository, create **`macos-v0.1.0-prerelease`, `macos-v0.1.0`, `macos-v0.1.1`** pointing at `ebbd9eee`, `64341e29`, `0cc4ca7f` (which are ancestors, section 0). The old `nightly` tags are moving tags: the unified `nightly` replaces them; the macOS `nightly` commit `6ab9f56a` is not in the work clone's object store (**missing**), so nothing points to it, and it need not.
- Release note policy: one release `v0.2.0` of the repository with all platforms' assets. The release notes: `kokai release` (what the old workflows used) lists the commits between the previous tag and the tag; with 1195 commits since `v0.1.41` this is useless as notes. Write them by hand (4.5); keep `kokai` only for `nightly`.

### 4.3 Artefact matrix

| Platform | Artefacts | Built by | Name pattern (keep stable for Scoop/WinGet) |
|---|---|---|---|
| Windows x86_64 and aarch64 | zip of the `.exe` files (komorebi, komorebic, komorebic-no-console, komorebi-bar, komorebi-gui, komorebi-shortcuts per `windows_targets` in `justfile`), MSI by `cargo wix -p komorebi -I .\wix\main.wxs`, `checksums.txt` | `windows-build` job (cargo, MSVC), not nix | `komorebi-<ver>-<target>.zip`, `komorebi-<ver>-<arch>.msi` as in windows.yaml; `wix/main.wxs` Product `UpgradeCode` `F8B967B5-...` must not change (a major upgrade keeps replacing the old install) |
| Scoop | manifest in ScoopInstaller/Extras (autoupdate by bot) | their bot, from `checkver: github` | needs the zip names above |
| WinGet | `LGUG2Z.komorebi` via `winget-releaser` on a published release | `winget` job | needs the MSI names |
| Linux (KDE) | zip of `komorebi`, `komorebic` for x86_64 and aarch64 (patchelf'ed to the system glibc loader, as the old `build` job did), `schema.json`; the **NixOS module** and overlay through the flake; no tarball existed | nix (native `komorebi`, `komorebic`), `ubuntu-24.04` and `ubuntu-24.04-arm` | `komorebi-<ver>-x86_64-unknown-linux-gnu.zip` |
| macOS aarch64 | zip of `komorebi`, `komorebic`, `komorebi-bar` (nix `result/bin/*`), `checksums.txt`, Homebrew formula, the nix-darwin module through the flake | nix on a Mac (native `komorebi`, `komorebic`, `komorebi-bar`) | `komorebi-<ver>-aarch64-apple-darwin.zip` (no leading `v`: the old release.yaml included it, `komorebi-v0.1.1-...`; choose the Windows form) |
| Schemas | `schemas/{windows,linux,macos}/schema*.json` attached to the release and published to the schema host | `just schemapub` (Cloudflare Pages `komorebi`, copies `schemas/<platform>/`) | per-platform files in the repository (decided); the published names are the open part of R5 |
| Docs | **Starlight for all three platforms** (owner, R3); `gh-pages` (MkDocs, `mkdocs gh-deploy`: the last commit on `gh-pages` is "Deployed 8f982e45 with MkDocs version 1.6.1", i.e. a manual deploy) is deprecated: steps in 4.9. The Starlight reference by `just docgen` (into `starlight/`) | by hand | root `mkdocs.yml` keeps `exclude_docs` `/linux/`, `/macos/`, `/dev/` (commit `4eae178f`) only while MkDocs still builds anything (4.9) |

`crates.io`: no crate is published today as far as the repository says; do not start with 0.2.0.

### 4.4 Secrets and holders

WinGet PAT (`WINGET_TOKEN`), Scoop (`SCOOP_TOKEN`, role **unknown**), a tap token (new, fine-grained), Cloudflare (schemas, only by hand today), none for signing (no signing exists). All held by the user. Agents never receive them.

### 4.5 What a user of each old project must change (the release notes skeleton)

Read from docs/dev/MIGRATION-SUMMARY.md and docs/dev/known-issues.md; what is NOT known is marked.
- **Windows 0.1.41 users.** The configuration file and `komorebic` commands are meant to be unchanged: MIGRATION-SUMMARY.md section 2 says the Windows state JSON "is unchanged by the shared Monitor/Workspace (platform extras are flattened in, pinned by tests)" and the schemas are pinned byte for byte; the CLI pages (`docs/cli`) were the oracle. Behaviour changes to mention: `komorebic visible-windows` now lists unreadable windows with empty strings (decision 14); the runtime `required` order in generated schemas may change (decision 11); windows rendering of the bar falls back to software OpenGL (wgpu) when OpenGL 2.0 is missing (decision 21). A shared `komorebi-client` still exports the same crate name. `cargo install --path komorebi` from a clone becomes `--path komorebi-daemon` (docs/installation.md:118). **Whether a user-visible change exists that the e2e did not catch: (unknown)**; the Windows e2e passed on cand9 except the known failures (below).
- **KDE users (the private repository: mostly the owner).** The repository, the flake input (`github:LGUG2Z/komorebi-for-kde` to `github:LGUG2Z/komorebi`), the module names (kept as aliases), `schema` location; `komorebic` for KDE got the pointer-picks-the-monitor preamble for `focus-workspace*` (known-issues.md section 1, decision 19) and the Windows-wording help texts of fifteen subcommands (progress log: "fifteen subcommands whose help differed join Messages with one help text"). Config: `StaticConfig` is one union type with per-platform views (decision 4); the KDE schema is checked in under `schemas/linux`.
- **macOS 0.1.x users.** Flake input `github:KomoCorp/komorebi-for-mac` (private) or `github:LGUG2Z/komorebi-for-mac` to `github:LGUG2Z/komorebi`; the Homebrew formula keeps its name for 0.2.0 (4.8); `$schema` URL of the config files (R5); `komorebi-bar` and `komorebic` are in the same zip as before; **behaviour changes that nobody has seen on a Mac**: hiding, the id registry, the shared Container API, settings snapshot, pointer picks the monitor, bar clicks (MIGRATION-SUMMARY.md section 5): all the subject of `docs/dev/macos-hand-check-plan.md`. Say so in the notes until the owner's hand-check is done (5.2).
- All: one version number across platforms; `komorebi --version` prints 0.2.0 on Windows, Linux and macOS (the three are one crate, `komorebi-version`); the macOS bar update check now points at the unified repository (class A above).
- Known issues to list (docs/dev/known-issues.md): items 2, 3 (inherited, pinned by tests), 4 (macOS subscriber stall); the Windows `commands` stall (about 1 in 10-12 sweeps, cause not found); `fullscreen` fails 2 checks on baseline too; the four stale e2e markers.

### 4.6 Steps (release phase only, after section 5's release gates; E1 and E2 come first)

E1. A revision `chore(version): 0.2.0` setting the workspace version to `0.2.0` and regenerating the schemas, `Cargo.lock` and the nix options; re-run the gates (5.2) on it. E2. Write `docs/dev/RELEASE-NOTES-0.2.0.md` (4.5). Then:
1. `git tag -a v0.2.0 <sha>` and push the tag (the `release` workflow builds, with `draft: true` first: the old `release-dry-run` job already creates a draft release; keep the draft step). 2. Check the draft's assets against the pattern table, the checksums, run each binary's `--version` (the old Linux job did this on the runner). 3. Publish the release (the draft turns to published; `winget-releaser` runs on publish). 4. Push the Homebrew formula (formula `komorebi-for-mac`, `url` = the public asset, no token header; delete the "requires a private repository" caveat). 5. `just schemapub` for each platform (R5). 6. `mkdocs gh-deploy`; deploy the Starlight reference. 7. The macOS mirror release in `komorebi-for-mac` (2.5 step 2). 8. Announce (the owner's channels, Discord is named in the README: `README.md` "you can ask on Discord"; nothing is posted by an agent). 9. Close or update the open PR comments; flip the README "not decided yet" text.

### 4.7 Verify, roll back, irreversible

Verify: `gh release view v0.2.0`, install each channel on a clean machine (Windows: the MSI over 0.1.41 as a major upgrade, which `wix/main.wxs` `MajorUpgrade` supports; Linux: the NixOS module in a VM (the VM suite `module` covers it); macOS: the formula on a Mac that is not the live one). Roll back: delete the draft; **after publish**: delete the release and tag only if no bot has run; mark `Pre-release` rather than delete if one has. **Irreversible once consumed:** a published tag and release that Scoop (autoupdate), WinGet (PR to winget-pkgs) and any nixpkgs bot picked up; downloaded installers; announcement posts.

### 4.8 The macOS nightly from the joined repository (release phase; NOT in the first CI)

Owner: "the komorebi for Mac nightly survives from the joined repo"; there is no private macOS repository any more, so the nightly build and its Homebrew formula must come from workflows of this repository. Starting point: `docs/dev/old-ci/macos/workflows/nix.yaml` (the `nightly` steps on master schedule/dispatch) and `formula.template.rb`. The old mechanism, read:
1. build on a Mac (`nix flake check`, `nix build`; there `runs-on: self-hosted`), `zip -j komorebi-nightly-aarch64-apple-darwin.zip result/bin/*`, `checksums.txt`;
2. tag and notes by `kokai release ... --ref nightly`; `gh release delete nightly`, `git push origin :nightly`, `gh release create nightly --prerelease` with the zip and checksums;
3. look up the asset id and sha256, `sed` them into `formula.template.rb` (`REPLACE_ASSET_ID`, `REPLACE_SHA_256`, `REPLACE_NIGHTLY`), upload the formula to the release, clone `LGUG2Z/homebrew-tap` with `GH_USER_API_TOKEN`, copy `komorebi-for-mac-nightly.rb` into `Formula/`, commit as "JeezyBot", push.
What must change: (a) **one `nightly` release for the whole repository**: the Windows workflow already owns the prerelease `nightly` (`windows.yaml` job `nightly`) and the old KDE one the same name, and each of them runs `gh release delete nightly` first, so three independent jobs would delete each other's assets. One workflow with a final job that downloads the artefacts of all platforms and creates the release once; (b) the formula's `url` can be the public asset URL: drop the `headers:` token lines and the caveat about a private repository (formula.template.rb:4-9, 25-31); keep the formula name `komorebi-for-mac-nightly` and `bin.install` plus the three `codesign --force -s -` lines so existing nightly users keep updating; (c) the Mac runner: hosted `macos-14` can build with nix **(inferred, not tried)**; the old one was self-hosted; (d) a fine-grained token limited to `LGUG2Z/homebrew-tap` (contents write) replaces `GH_USER_API_TOKEN`; (e) the gist deploy of `schema.json` (`exuanbo/actions-deploy-gist`, gist `78b3dd12...`) is replaced by the schema publication decided in R5, not copied. The macOS nightly lives in the release workflow (A.3), starts after the owner has run the hand-check, and is added to CI in the release phase.

### 4.9 Retiring the `gh-pages` site (R3: Starlight for all docs; gh-pages deprecated)

Facts: Pages serves branch `gh-pages` at `https://lgug2z.github.io/komorebi/` (`gh api repos/LGUG2Z/komorebi/pages`); the branch is a manual `mkdocs gh-deploy` result; the old site is linked from `README.md`, `komorebic/src/windows.rs` and `komorebic/src/macos/mod.rs` (help text), `.github/ISSUE_TEMPLATE/config.yml` and `bug_report.yml`, and from every installed 0.1.x binary's help. Where the guide pages (installation, usage, common-workflows) are the source of truth after the move, in this repository's `docs/` or in the Starlight site: **(unknown)**; the site is `starlight/` in this repository now (the `komorebi-starlight` history is merged in), and `just docgen` only generates the CLI pages and schema reference pages into it.
Steps (owner): 1. Starlight has the guides of all three platforms (Windows from `docs/`, Linux from `docs/linux/`, macOS from `docs/macos/`) and its build passes. 2. Retarget the links above (and the `$schema`/docs links of 2.3 class F) to the Starlight address. 3. Replace the `gh-pages` content by a one-page redirect to Starlight and leave it for a period the owner chooses (old binaries and bookmarks still point to it), then disable Pages in the settings, then delete the branch last (back it up as a tag first; deleting is the only irreversible part). 4. **`mkdocs.yml` and the Windows docs source:** while MkDocs builds anything, the root `mkdocs.yml` keeps `exclude_docs` for `/linux/`, `/macos/` and `/dev/` (commit `4eae178f`) so a MkDocs build still excludes the other platforms' pages; when the last MkDocs build is gone, delete `mkdocs.yml`, `check_schema_docs.py` if it only serves it (check first), and decide whether the Windows Markdown under `docs/` stays as in-repo documentation (recommended: keep it, it is what `komorebic` and README link to and what Starlight can import) or moves wholly into Starlight.

## 5. Sequencing and gates

### 5.1 Order of the irreversible steps

**Phase 1: DEV-LINE LANDING (`0.2.0-dev`, no tag, no release artefacts, CI on)**

| # | Step | Reversible? | Gate before it (go/no-go) |
|---|---|---|---|
| L0 | The macOS token is not rotated (owner decision); N1 (nextest) is answered (3.3); the R6 text rewrites (2.2a) are done | yes | the user's answers |
| L1 | `master` is already frozen (done by the owner). Merge the ready PRs, post the path-map comment on the rest, close the 8 dependabot PRs; record the `master` sha | yes | `master` sha unchanged since the freeze |
| L2 | Scratch-repo dry run (5.4) of push and CI (the release part is rehearsed later, E0) | yes (delete the scratch repository) | CI files drafted |
| L3 | Push `komomono` (fast-forward) | **partly**: the private KDE and macOS histories become public | the owner confirms the push itself (decision 1) |
| L4 | Review on GitHub, then push `master` (fast-forward) | no (see 1.9) | gates of 5.2 (dev-line column) green on the exact commit |
| L5 | Ruleset, `legacy/windows-0.1` branch; CI runs on `master` | yes | |
| L6 | (nothing for the old macOS repository: the owner handles it, R7.) The old KDE repository stays as it is until the release | yes | after L5 |

**Phase 2: RELEASE of 0.2.0 (later, when the owner decides)**

| # | Step | Reversible? | Gate before it (go/no-go) |
|---|---|---|---|
| E0 | Dry run of the release (draft, `-rc` tag on the scratch repo) | yes | CI green on `master` for the dev line |
| E1 | Revision setting version `0.2.0`, regenerate schemas and lock | yes until pushed | gates of 5.2 (release column) |
| E2 | Release notes | yes | |
| E3 | Tag `v0.2.0`, draft release | draft yes | full Windows e2e and KDE `fast`+`slow` on the tagged commit; the macOS hand-check done by the owner (R2); the owner re-opens R4 |
| E4 | Old KDE repository: README banner, close #6, archive (owner) | yes | after E5 |
| E5 | Publish; WinGet/Scoop/schemas/Starlight docs; the macOS nightly formula (4.8); enable `nightly`; retire `gh-pages` (4.9) | **no once consumed** | the release checklist in 5.5 |
| E6 | Announce | no | after E5 verified |

### 5.2 Gates on the final tree, and the open findings

The gates that exist: Windows e2e (QEMU, section 3), KDE VM `fast` and `slow`, the nix check groups of each platform, native tests (`cargo test --workspace` on Linux: 96 test binaries per the progress log), macOS compile/clippy/unit tests on the Mac Mini (665 passed, 0 failed, 1 ignored at candidate 8). State at the last landing (progress log 2026-11-01 01:08): all gates green on `wip-cand9b` (`21cf0d73` = `wip-main`): KDE VM cand9 16/16, Windows targeted gate cand9 22 scenarios green but for the stale `maximizing.user` marker, nix Windows and KDE groups and packages pass, Mini cand9b green. **What was NOT run on the final tree**: the KDE `slow` tier (18 suites) was never run on any candidate (MIGRATION-SUMMARY.md section 5), the full 47-scenario Windows set on cand9/the cutover stack (cand8 ran it; cand9 ran 22), anything on the cutover stack (renames, version 0.2.0, docs, and the flake rewrite): the renames moved every directory so **every gate must be re-run once on the tip**, since a rebase invalidates a claim (decision 26). The progress log records "verified: Windows clippy, Linux tests 96 binaries, macOS fake-CC check" for the cutover stack, not the e2e or VM or nix builds.

**The macOS hand-check pass** (`docs/dev/macos-hand-check-plan.md`, about 5 hours on a real Mac, never run; blocks 0-8, table line 288 "total about 5 hours"). Owner decision (R2): **macOS is part of the dev line (`0.2.0-dev`); the owner tests it himself and releases 0.2.0 only when it is ready.** So the pass gates only the 0.2.0 RELEASE (E3), not the dev-line landing; the earlier idea of a separate macOS pre-release is dropped. The macOS code has never run as a daemon in these builds (MIGRATION-SUMMARY.md section 5) and changed substantially (shared Container, registry, settings, hiding): that is exactly what the pass checks (blocks 0 to 4, about 2.7 hours, are the "could make the daemon unusable" ones: 25+40+45+40+10 min per the block headers). **Gates by phase:** dev line = the existing green gates re-run on the tip after the renames and the flake (Windows e2e targeted set, KDE VM `fast`, nix groups, native tests, Mac Mini compile/unit); known failures do not block it (R4); release = the dev-line gates plus the full 47-scenario Windows set, KDE `slow`, and the macOS pass.

**Open findings as release blockers or not** (docs/dev/known-issues.md, build-and-test.md section 6, progress log):
- not blockers (inherited behaviour, pinned by tests): known-issues 2 (`Monitor::focus_workspace` overwrites a name), 3 (`remove_container` focus), 4 (macOS subscriber with no write timeout), mention in notes;
- **R4 (owner): known failures do not block the dev-line landing; for the release the owner re-opens the question. Meanwhile:** not a blocker but should be marked, not left failing: the four stale e2e markers and `fullscreen` (fails the same 2 checks on baseline, cand1, cand5, cand8: "does not take the window out of fullscreen", "not on any other window"; pre-existing, not a regression): re-mark so a green nightly is possible (R4);
- **blocker-candidate:** the Windows lock-hold stall (`commands` stops answering `komorebic state` for 3 to 3.6 s about 1 in 10-12 sweeps; longest hold 5746 ms at cand9 vs 8250 at cand5; cause not found): it exists on the baseline (1/10 before the consolidation) so it is not a regression; ship with a known-issue entry (R4);
- `known-issues.md` itself has no entry for `fullscreen` yet (grep of the file shows none): add it before release.

### 5.3 The git commands (run by the owner only: "yes, but I will push myself"; agents never run them)

All commands assume `~/komorebi` is the user's repository (jj colocated), `origin` = GitHub. They are written down, not run: the OWNER runs every push and every tag command; agents prepare them and never run them.

```sh
# L1  record the base (master is already frozen by the owner; nothing is pushed)
git -C ~/komorebi fetch origin
git -C ~/komorebi rev-parse origin/master                     # expect 72aa4c97ade72b802a008bdba4bf59cca04df665 (or newer: then merge, do not fast-forward)
gh api repos/LGUG2Z/komorebi/branches/master --jq .commit.sha

# L3  (before the push) import the final stack as ONE bookmark (D1 in the decisions log describes the work clone)
cd ~/komorebi
jj git remote add work /home/jado/komomono-work/komorebi        # once
jj git fetch --remote work --branch wip-main                          # wip-main = 6067938e at the time of writing; re-read it
jj log -r 'komomono@origin..wip-main@work' --no-graph -T 'commit_id.short()++"\n"' | wc -l   # 1151 at wip-main 6067938e (komomono@source..wip-main)
jj log -r 'wip-main@work..komomono@origin'             # must be EMPTY (the local line contains origin's komomono)

# L3  push komomono: a fast-forward of origin's komomono, never --force
jj bookmark set komomono -r wip-main@work               # a forward move; jj refuses a sideways move without --allow-backwards
jj git push --bookmark komomono                                  # pushes ONLY this bookmark
# equivalent with git, as a double check that it is a fast-forward:
git merge-base --is-ancestor origin/komomono "$(jj log -r komomono --no-graph -T commit_id)" && echo fast-forward

# L4  after sign-off: master gets the same commit (fast-forward)
jj bookmark set master -r komomono
jj git push --bookmark master                                    # rejected if master moved: STOP, do not force
git branch legacy/windows-0.1 72aa4c97ade72b802a008bdba4bf59cca04df665
git push origin legacy/windows-0.1

# E3  (RELEASE phase only, never during the dev line) the tag
git tag -a v0.2.0 -m "komorebi 0.2.0" "$(jj log -r master --no-graph -T commit_id)"
git push origin v0.2.0
# macOS historic tags, optional:
git tag macos-v0.1.1 0cc4ca7f67d79da0d59d603dde2f6b4445fec85b && git push origin macos-v0.1.1
```
`jj git push --bookmark master` refuses a non-fast-forward move by default and prints the reason; if it does, do not add `--allow-...` flags. Check before each push with `jj git push --dry-run --bookmark komomono` (jj 0.2x+; **the installed jj version was not checked**, `git push --dry-run origin <sha>:refs/heads/master` is the git equivalent).

### 5.4 Dry run on a scratch repository (proposed, not created)

Create (by the user) a private repository `LGUG2Z/komorebi-dryrun` (or a fork under another account, since forks of a public repo are public) and: (1) push the same single commit as `master`, two tags `v0.2.0-rc1` and `nightly`; (2) add the Actions secrets as dummies (`WINGET_TOKEN=dummy`) so that jobs which publish fail at the publish step, not before; (3) give it the self-hosted runner label, a Linux KVM-capable hosted runner (private repositories may land on a host with no `/dev/kvm`: that is the failure `rerun-no-kvm` was written for), a macOS runner; (4) run the whole `ci` once, the nightly, a release from the `-rc1` tag with `draft: true`; (5) check the checksum file with Scoop's rule by running `scoop`'s `checkver`/`autoupdate` locally (`checkver.ps1` against the scratch release) and `winget-releaser` in `--dry-run` style input if available **(unknown)**; (6) `git merge-tree` of each of the 19 open community PR heads against the pushed `master` to count clean merges; (7) delete the repository. Cost: private-repo Actions minutes are billed on a Free plan: **(unknown)**; to avoid it make it public for the hour and delete it, but that publishes KDE history: do not do that before the owner has confirmed the push (L3). A tree with the KDE source removed would not be the tree under test.

### 5.5 Go / no-go checklists

**Dev-line landing checklists**
Before **L3** (push `komomono`, run by the owner): the owner confirms the push itself (the KDE and macOS private histories become public); the gitleaks result is on record (section 0: default rules, no leaks, limits) and a custom pass for `KomoCorp`, internal hostnames and the token URL form returned nothing (**not run: run it**); the old-hash script (1.7) on the work clone gives four `ok`; `jj log -r 'wip-main@work..komomono@origin'` empty; the docs name the flake outputs of `docs/dev/flake.md` (the flake has landed); the version on the line is `0.2.0-dev`; no secrets in the pushed tree (`git grep -nIE '(ghp_|github_pat_|x-access-token)' <sha>` returns nothing: **not run, run it**) and no large artefacts tracked.
Before **L4** (push `master`): L3 reviewed in the GitHub compare view; CI files dry-run green on the scratch repo with the `nightly` publish and any release/winget/tap job disabled or unable to fire (no `v*` tag; `nightly` publish step off); the dev-line gates of 5.2 re-run on the tip commit and logged; the PR triage done; `master` sha unchanged since L1; a person is ready to watch the first CI run. Neither the macOS hand-check nor the known failures (R4) block this landing.

**Release checklists (later)**
Before **E3** (tag): E1 revision gated again on its own tip (a rebase invalidates a claim); full Windows e2e (47 scenarios) and KDE VM `fast` and `slow` on the exact commit; the macOS hand-check (blocks 0 to 8, at least 0 to 4) done by the owner and logged; the owner has re-opened R4 and decided what the known failures mean for the release; known issues entries written; release notes written.
Before **E5** (publish): draft assets match 4.3; checksums verified; MSI major-upgrade install tested over 0.1.41 in the Windows e2e guest or a VM; the Linux zip's binaries run on a plain Ubuntu runner (`--version`); the macOS artefact and nightly formula exercised on a Mac by the owner; the notes include the migration skeleton (4.5) and known issues; `WINGET_TOKEN` valid; the Starlight site builds with all three platforms' docs.

## 6. Risks and open decisions for the user (answer in one line each)

Decided by the owner (kept for the record): **R1** no private sponsors repository, publish the KDE and macOS history, the owner pushes every time; **R2** macOS is part of 0.2.0-dev, the owner tests it and releases 0.2.0 only when it is ready; **R3** Starlight for all docs, `gh-pages` deprecated; **R4** known failures do not block the dev-line landing (re-opened by the owner for the release); **R5** per-platform schema files, no root copies, published names `komorebi.<platform>.<version>.schema.json` and `komorebi.bar.<platform>.<version>.schema.json` on the existing Cloudflare Pages project; **R6** one sponsor-only `[FEAT]` policy and one licence, texts rewritten by the agent `sponsor-texts`; **R7** the owner handles the old macOS repository (this plan has no step for it); **R8** `master` is already frozen; **R9** hosted Linux and macOS first, self-hosted Windows runner later; **R10** PR only; **R11** removed: there is no upstream nixpkgs package, the flake of this repository is the Nix distribution; **R12** GitHub cache with per-platform keys, no Cachix. The macOS nightly (Homebrew formula and build) survives from the joined repository (4.8); the macOS token is not rotated (owner decision).

**What is truly open now:**
1. **The push itself (owner).** The owner runs L3 (`komomono`) and L4 (`master`), then L5 (ruleset, `legacy/windows-0.1`), using the commands of 5.3; confirm when.
2. **The macOS hand-check (owner; gates only the 0.2.0 RELEASE).** About 5 hours on a real Mac (`docs/dev/macos-hand-check-plan.md`); macOS is on the dev line meanwhile; the owner releases 0.2.0 when it is ready.
3. **The self-hosted Windows runner (later).** KVM, the evaluation ISO (expires every 90 days), then the Windows e2e, KDE VM tiers and canary as nightly/manual jobs (A.2).
4. **N1, the nextest decision: answered** (3.3): macOS nextest runs in nix on a Mac (383 tests); the Windows unit tests run in the Windows VM through `nix run .#windows-nextest` (964 tests, self-hosted phase); the neutral crates' tests run in `linux-nextest` (1532). Nothing is left to decide except whether to add a hosted `windows-latest` `cargo test` job to the first CI (not needed for the dev-line landing).
5. **Release-time items** (decided at the release, not now): re-open the known failures (R4: the four stale e2e markers, `fullscreen`, the lock-hold stall) and whether they block; the release notes (4.5); the full gates (5.2); the macOS nightly workflow and formula (4.8); retiring `gh-pages` (4.9); the old KDE repository archive (2.5).

What could not be determined: active user counts, the macOS repo's 13 "open issues", the cost of hosted minutes, the hosted-runner wall time of the KDE suites, why the KDE `ci` push runs fail in 7 to 10 seconds, whether Chocolatey or other third-party packages exist, the role of `PAT` and `SCOOP_TOKEN`, DNS for `komorebi.lgug2z.com` and `komorebi-for-mac.lgug2z.com`, the real names of the flake outputs (placeholders until `flake-one` lands), whether the 19 community PRs merge cleanly after the renames, the installed jj version.

## Appendix A. Drafts of the workflow files (text only; nothing was created under `.github/`)

Output names are the real ones of the root flake at `wip-main` (`6067938e`; docs/dev/flake.md and docs/dev/flake-checks.md, `nix/checks.nix`): checks are
`.#checkSets.<set>` with the sets `fmt`, `deny`, `windows-lint`, `windows-test`, `linux-lint`, `linux-test`, `linux-options`, `macos-lint`, `macos-test`,
`macos-options` (together `cheap`, what `nix flake check` and the pull request run), the heavy sets `vm-fast`, `vm-slow`, `linux-nixos-eval`, `linux-demo`, and `all`; one member:
`.#checkMembers.<set>.<name>` or `.#checks.<system>.<name>` for a default-set member. Packages: native `komorebi`, `komorebic`, `komorebi-bar`, `komorebi-full`, `default` on the Linux and Darwin
systems, and `windows-komorebi*` (cross-built on Linux); VM suites `legacyPackages.<system>.vm.<name>`, tiers `legacyPackages.<system>.vmTiers`; the Windows e2e app `nix run .#e2e-windows`. Pin actions by tag as the old files did
(`actions/checkout@v7`, `actions/upload-artifact@v7`, `actions/download-artifact@v8`, versions as in the old workflows and in the dependabot PRs open on the Windows repository).
Not verified by running: the exact set contents are read from the docs; evaluate `nix eval --json .#legacyPackages.x86_64-linux.checkSets --apply builtins.attrNames` once before using the names.

### A.1 `.github/workflows/ci.yaml` (the first CI: hosted Linux and hosted macOS only; PR only plus `master`)

```yaml
name: ci

on:
  pull_request:
  push:
    branches: [master]            # no feature/* or hotfix/* triggers (R10: PR only)
  workflow_dispatch:

concurrency:
  group: ci-${{ github.event_name == 'pull_request' && github.ref || github.sha }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

permissions:
  contents: read

jobs:
  deny:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v7
      - uses: EmbarkStudios/cargo-deny-action@v2

  # The pull request gate: the cheap check set (fmt, deny, windows-lint, windows-test, linux-lint, linux-test,
  # linux-options, macos-lint/test/options members that exist on x86_64-linux). One derivation, built as one job.
  # If one runner runs out of resources (the old KDE job did, at about six minutes), split by set (matrix below).
  cheap:
    runs-on: ubuntu-24.04
    strategy:
      fail-fast: false
      matrix:
        set: [fmt, deny, windows-lint, windows-test, linux-lint, linux-test, linux-options]
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - uses: ./.github/actions/nix
      - run: nix build -L --no-link .#checkSets.${{ matrix.set }}

  # macOS: build, lint and the darwin nix checks. No daemon is started (no accessibility grant, no display in CI).
  # `macos-nextest` runs no test today (see the nextest decision N1), so unit tests need `cargo test` below.
  macos:
    runs-on: macos-14                     # hosted arm64; a self-hosted Mac is a later option
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - uses: ./.github/actions/nix
      - run: nix build -L --no-link .#checkSets.macos-lint .#checkSets.macos-test .#checkSets.macos-options .#checkSets.fmt .#checkSets.deny
      - run: nix build -L .#komorebi .#komorebic .#komorebi-bar
      - uses: actions/upload-artifact@v7
        with:
          name: komorebi-aarch64-apple-darwin-${{ github.sha }}
          path: result*/bin/komorebi*
          retention-days: 14

  linux-build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { target: x86_64-unknown-linux-gnu, runs-on: ubuntu-24.04, loader: /lib64/ld-linux-x86-64.so.2, optional: false }
          - { target: aarch64-unknown-linux-gnu, runs-on: ubuntu-24.04-arm, loader: /lib/ld-linux-aarch64.so.1, optional: true }
    runs-on: ${{ matrix.runs-on }}
    continue-on-error: ${{ matrix.optional }}
    steps:
      - uses: actions/checkout@v7
      - uses: ./.github/actions/nix
      - env: { LOADER: "${{ matrix.loader }}" }
        run: |
          mkdir bin
          for name in komorebi komorebic; do
            cp "$(nix build -L --no-link --print-out-paths ".#$name")/bin/$name" bin/
          done
          chmod u+w bin/*
          nix run --inputs-from . nixpkgs#patchelf -- --set-interpreter "$LOADER" --remove-rpath bin/*
          bin/komorebi --version
          bin/komorebic --version
      - uses: actions/upload-artifact@v7
        with: { name: "komorebi-${{ matrix.target }}-${{ github.sha }}", path: bin/, retention-days: 14 }

  # Windows binaries are cross-built on Linux by nix (windows-komorebi*); the hosted windows-latest cargo job of the old
  # workflow is NOT in the first CI (owner decision R9). See N1 for what that leaves untested.
  windows-cross:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - uses: ./.github/actions/nix
      - run: nix build -L --no-link .#windows-komorebi .#windows-komorebic .#windows-komorebi-bar

  # The one required check (the branch ruleset names only this job).
  required:
    if: always()
    needs: [deny, cheap, macos, linux-build, windows-cross]
    runs-on: ubuntu-24.04
    steps:
      - run: |
          echo '${{ toJSON(needs) }}' | jq -e 'to_entries | all(.value.result == "success" or .value.result == "skipped")'
```

### A.2 `.github/workflows/sweeps.yaml` (LATER PHASE: needs a KVM-capable self-hosted runner; not part of the first CI)

Owner decision R9: the owner will self-host later. Until a self-hosted runner with KVM exists this file is not added. Then the VM tiers and the Windows e2e become
nightly and manual jobs (not PR gates; the PR gate stays `checkSets.cheap`).

```yaml
name: sweeps

on:
  schedule:
    - cron: "15 1 * * *"                  # nightly; the full Windows set on Sundays
  workflow_dispatch:
    inputs:
      what:
        description: "vm-fast | vm-slow | e2e-windows:<scenarios or full>"
        default: "vm-fast"

permissions:
  contents: read

jobs:
  discover:
    runs-on: [self-hosted, linux, kvm]
    outputs:
      fast: ${{ steps.tiers.outputs.fast }}
      slow: ${{ steps.tiers.outputs.slow }}
    steps:
      - uses: actions/checkout@v7
      - id: tiers
        run: |
          tiers=$(nix eval --json .#legacyPackages.x86_64-linux.vmTiers)
          echo "fast=$(jq -c '.fast // []' <<<"$tiers")" >> "$GITHUB_OUTPUT"
          echo "slow=$(jq -c '.slow // []' <<<"$tiers")" >> "$GITHUB_OUTPUT"

  vm:
    needs: discover
    runs-on: [self-hosted, linux, kvm]
    timeout-minutes: 60
    strategy:
      fail-fast: false
      matrix:
        suite: ${{ fromJson(needs.discover.outputs.fast) }}      # add the slow tier on a weekly schedule
    steps:
      - uses: actions/checkout@v7
      - run: nix build -L .#legacyPackages.x86_64-linux.vm.${{ matrix.suite }}      # or .#checkMembers.vm-fast.vm-<suite>

  e2e-windows:
    # the owner's machine; evaluation ISO expires every 90 days (e2e/platforms/qemu_windows/README.md:20-24): rebuild the base image then
    runs-on: [self-hosted, linux, kvm-windows-e2e]
    timeout-minutes: 240
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - env:
          E2E_GOLDEN: /srv/komorebi-e2e/golden
        run: |
          set -eu
          nightly="commands workspaces focus stacks monocle floating maximizing rules rules_class rules_exe manage_rules startup_rules title_rule layouts config_reload lifecycle handover resize lifecycle_hidden hotplug promoting"
          nix run .#e2e-windows -- "$RUNNER_TEMP/out" $nightly      # the full 47 names on Sundays: list them; "no names = all" is not verified
      - if: always()
        uses: actions/upload-artifact@v7
        with: { name: "e2e-${{ github.run_id }}", path: "${{ runner.temp }}/out/**/result.json", retention-days: 14 }
```
Never run fork code on the self-hosted runner (no `pull_request` trigger here for that reason).

### A.3 `.github/workflows/release.yaml` (RELEASE phase: leave this file out of the dev-line landing, or keep only the `ci` call; the `schedule` and `v*` triggers and the `nightly` publish must not fire until the release phase)

The Windows artefacts of the release (MSI by `cargo wix`, zips) need a Windows host: add, in the release phase only, the `windows-build` job of the old workflow (hosted `windows-latest`, two targets, CRLF check, `cargo +nightly fmt --check`, clippy, `cargo test`, release build, `cargo wix --no-build -p komorebi ... -I .\wix\main.wxs`; see 3.2) as a job of `release.yaml` that this file's `publish` job `needs`. The `ci` call below is the Linux/macOS one.

```yaml
name: release

on:
  push:
    tags: ["v*"]
  schedule:
    - cron: "30 0 * * 0"                  # the weekly nightly, as before
  workflow_dispatch:

permissions:
  contents: read

jobs:
  ci:
    uses: ./.github/workflows/ci.yaml     # needs `on: workflow_call` added to ci.yaml

  publish:
    needs: ci
    runs-on: ubuntu-24.04
    permissions:
      contents: write
    env:
      GH_TOKEN: ${{ github.token }}
      GH_REPO: ${{ github.repository }}
    steps:
      - uses: actions/checkout@v7
        with: { fetch-depth: 0 }
      - run: |
          if [[ "$GITHUB_REF" == refs/tags/v* ]]; then
            echo "TAG=$GITHUB_REF_NAME" >>"$GITHUB_ENV"; echo "VERSION=${GITHUB_REF_NAME#v}" >>"$GITHUB_ENV"
          else
            echo "TAG=nightly" >>"$GITHUB_ENV"; echo "VERSION=nightly" >>"$GITHUB_ENV"
          fi
      - uses: actions/download-artifact@v8
        with: { pattern: "komorebi-*-${{ github.sha }}", path: artifacts }
      - name: Package (Windows zips and MSIs, Linux zips, macOS zip) with the names Scoop and WinGet expect
        shell: bash
        run: |
          mkdir dist
          for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
            d="artifacts/komorebi-$t-$GITHUB_SHA"
            (cd "$d/$t/release" && zip -j "$GITHUB_WORKSPACE/dist/komorebi-$VERSION-$t.zip" *.exe)
            cp "$d"/wix/*"${t%%-*}".msi "dist/komorebi-$VERSION-${t%%-*}.msi"
          done
          for t in x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
            d="artifacts/komorebi-$t-$GITHUB_SHA"; [ -d "$d" ] || continue
            chmod +x "$d"/*; zip -j "dist/komorebi-$VERSION-$t.zip" "$d"/komorebi "$d"/komorebic
          done
          d="artifacts/komorebi-aarch64-apple-darwin-$GITHUB_SHA"
          chmod +x "$d"/*; zip -j "dist/komorebi-$VERSION-aarch64-apple-darwin.zip" "$d"/*
          cp schemas/windows/schema.json dist/schema.windows.json
          cp schemas/linux/schema.json   dist/schema.linux.json
          cp schemas/macos/schema.json   dist/schema.macos.json
          (cd dist && sha256sum -- *.zip *.msi *.json > checksums.txt)
      - name: Release notes
        run: |
          if [ "$TAG" = nightly ]; then
            cargo install --locked kokai@0.3.1
            git tag --force nightly
            kokai release --no-emoji --add-links github:commits,issues --ref nightly >"$RUNNER_TEMP/notes.md"
          else
            cp docs/dev/RELEASE-NOTES-$VERSION.md "$RUNNER_TEMP/notes.md"      # written by hand (CUTOVER-PLAN 4.5)
          fi
      - name: Publish
        run: |
          if [ "$TAG" = nightly ]; then
            gh release delete nightly --cleanup-tag --yes || true
            gh release create nightly --target "$GITHUB_SHA" --prerelease --title "komorebi nightly ($GITHUB_SHA)" \
              --notes-file "$RUNNER_TEMP/notes.md" dist/*
          else
            gh release create "$TAG" --verify-tag --draft --title "komorebi $TAG" --notes-file "$RUNNER_TEMP/notes.md" dist/*
            # the owner reviews the draft and publishes it by hand; WinGet runs on publish
          fi

  winget:
    needs: publish
    if: github.event_name == 'release' && github.event.action == 'published'     # moved to its own workflow `on: release: published` when drafts are used
    runs-on: ubuntu-latest
    steps:
      - uses: vedantmgoyal2009/winget-releaser@main
        with:
          identifier: LGUG2Z.komorebi
          token: ${{ secrets.WINGET_TOKEN }}
```
The `winget` job as written never runs from this file: a draft published by hand fires `release: published`, so it belongs in its own workflow file with that trigger. The Homebrew push is a second job in that file, using a fine-grained token limited to `LGUG2Z/homebrew-tap` and a formula without the token header (the old `formula.template.rb` replaced `REPLACE_ASSET_ID` and `REPLACE_SHA_256` with `sed`; keep that mechanism, replace `url` by the public download URL).

### A.4 `.github/workflows/update-flake-lock.yaml`, `rerun-no-kvm.yaml`, `canary.yaml`

`update-flake-lock.yaml` and `rerun-no-kvm.yaml`: the old files unchanged (docs/dev/old-ci/linux/workflows/), with the `if: github.repository == 'LGUG2Z/komorebi-for-kde'` guards changed to `LGUG2Z/komorebi`, and `workflows: [ci]` kept in `rerun-no-kvm`. `canary.yaml`: the old `canary` job split by platform group, with the `nixpkgs` move done in a copy of the checkout as before (docs/dev/old-ci/linux/workflows/ci.yaml `canary`, 360-minute timeout) and the issue title "Canary: a check fails against the tip of nixpkgs-unstable" kept so the existing issue logic keeps working.

### A.5 `.github/dependabot.yml`

```yaml
version: 2
updates:
  - package-ecosystem: "github-actions"
    directory: "/"
    schedule: { interval: "weekly" }
    assignees: ["LGUG2Z"]
    commit-message: { prefix: chore, include: scope }

  - package-ecosystem: "cargo"
    directory: "/"
    schedule: { interval: "weekly" }
    assignees: ["LGUG2Z"]
    commit-message: { prefix: chore, include: scope }
    groups:
      cargo-minor-patch:
        update-types: ["minor", "patch"]
# flake.lock is updated by .github/workflows/update-flake-lock.yaml (weekly); the `nix` ecosystem is not enabled
# so that two bots do not update the same file.
```

### A.6 `.github/actions/nix/action.yaml`

The old composite action (docs/dev/old-ci/linux/actions/nix/action.yaml) moves as is to `.github/actions/nix/action.yaml`; its cache key `hashFiles('flake.lock', '**/Cargo.lock')` already covers the one lock; the cache is decided (R12): `actions/cache` (through `cache-nix-action`) with a per-platform key prefix (`nix-linux-`, `nix-macos-`), no Cachix for now.

## Appendix B. Where the facts come from

`docs/dev/MIGRATION-SUMMARY.md` (sections 1, 4-7, 9), `docs/dev/known-issues.md`, `docs/dev/unified-binaries.md`, `docs/dev/build-and-test.md` (sections 1-6), `docs/dev/macos-hand-check-plan.md` (headers, line 288), `docs/macos/README.md:43-47`, `.github/workflows/*`, `docs/dev/old-ci/{linux,macos}/**`, `Cargo.toml:61`, `justfile` (`schemapub`, `windows_targets`), `wix/main.wxs`, `mkdocs.yml`, `e2e/platforms/qemu_windows/README.md`, `tools/e2e-needed`; the plan `generic-percolating-matsumoto.md` ("Decisions already made", Part A); the progress log `~/.claude/plans/komomono-progress.md` (the entries for the e2e gates, "ALL GATES GREEN on wip-cand9b", the cutover cleanup and docs-paths entries) and the decisions log; commands: `jj log -r ...` on `/home/jado/komomono-work/komorebi`, `git log/grep/show/blame/tag` on that clone and on the three old clones, `gh repo view`, `gh api` (repos, branches, rulesets, pages, secrets (names), actions/runs, releases, contents of `ScoopInstaller/Extras` and `NixOS/nixpkgs` and `LGUG2Z/homebrew-tap`), `gh issue list`, `gh pr list`, `gh release list`, `gh workflow list`, and one `curl -I` to `https://komorebi.lgug2z.com/komorebi.0.1.41.schema.json` (404).
