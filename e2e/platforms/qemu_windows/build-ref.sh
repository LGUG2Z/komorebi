#!/usr/bin/env bash
# Builds the binaries of another komorebi commit, to run the e2e scenarios
# against them: `KOMOREBI_E2E_BINS=OUTDIR nix run .#e2e-windows -- ...`.
#
# usage: build-ref.sh REF OUTDIR
# REF is anything git knows (a commit, a branch, a tag); it is fetched from
# origin if it is not here yet. The checkout of REF is built with this
# tree's flake (see below), because the nix build of an older commit may not
# work any more; the test window is this tree's. OUTDIR gets
# komorebi.exe, komorebic.exe, komorebi-testwin.exe and `commit`, the full
# hash, which the runner writes into every result.json.
# needs on PATH: git nix
set -euo pipefail

ref=$1 out=$2
here=$(cd "$(dirname "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)

if ! git -C "$root" cat-file -e "$ref^{commit}" 2>/dev/null; then
  git -C "$root" fetch origin "+refs/heads/*:refs/remotes/origin/*"
fi
commit=$(git -C "$root" rev-parse "$ref^{commit}")

tree=$(mktemp -d)
cleanup() {
  git -C "$root" worktree remove --force "$tree" 2>/dev/null || rm -rf "$tree"
}
trap cleanup EXIT
git -C "$root" worktree add --detach "$tree" "$commit"
# A REF of the layout with one root flake (nix/windows/) is built with this tree's flake.nix, flake.lock and
# nix/, whose Windows outputs are `windows-NAME`. An older REF (the Windows flake with `komorebi/` and the
# nested flakes) keeps its own flake: its outputs are `NAME`, and its file layout is not one this tree's
# flake can read.
if [ -e "$tree/nix/windows/flake-module.nix" ]; then
  rm -rf "$tree/nix"
  cp -r "$root/nix" "$tree/nix"
  cp "$root/flake.nix" "$root/flake.lock" "$tree/"
  prefix=windows-
else
  prefix=
fi

mkdir -p "$out"
for package in komorebi komorebic komorebi-bar; do
  path=$(nix build "$tree#$prefix$package" --no-link --print-out-paths)
  cp "$path/bin/$package.exe" "$out/"
done
path=$(nix build "$root#windows-komorebi-testwin-e2e" --no-link --print-out-paths)
cp "$path/bin/komorebi-testwin.exe" "$out/"
chmod u+w "$out"/*.exe
echo "$commit" > "$out/commit"
echo "built $commit into $out"
