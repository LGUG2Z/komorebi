#!/usr/bin/env bash
# Builds the binaries of another komorebi commit, to run the e2e scenarios
# against them: `KOMOREBI_E2E_BINS=OUTDIR nix run .#e2e-windows -- ...`.
#
# usage: build-ref.sh REF OUTDIR
# REF is anything git knows (a commit, a branch, a tag); it is fetched from
# origin if it is not here yet. The checkout of REF is built with this
# tree's flake.nix and flake.lock, because the nix build of an older commit
# may not work any more; the test window is this tree's. OUTDIR gets
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
cp "$root/flake.nix" "$root/flake.lock" "$tree/"

mkdir -p "$out"
for package in komorebi komorebic; do
  path=$(nix build "$tree#$package" --no-link --print-out-paths)
  cp "$path/bin/$package.exe" "$out/"
done
path=$(nix build "$root#komorebi-testwin" --no-link --print-out-paths)
cp "$path/bin/komorebi-testwin.exe" "$out/"
chmod u+w "$out"/*.exe
echo "$commit" > "$out/commit"
echo "built $commit into $out"
