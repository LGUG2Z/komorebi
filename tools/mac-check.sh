#!/usr/bin/env bash
# usage: tools/mac-check.sh [extra cargo args]
# Type-check of the macOS code on Linux (target aarch64-apple-darwin) through the fake C compiler
# tools/cc-fake.sh. Cannot link or run anything. Same command as the macOS step of tools/fast-check, but
# unconditional. Run from anywhere inside the repository; uses FAST_CHECK_SCRATCH like fast-check.
set -euo pipefail
S="${FAST_CHECK_SCRATCH:-${XDG_CACHE_HOME:-$HOME/.cache}/komorebi-fast-check}"
HERE="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
ROOT="$(jj root 2>/dev/null || git rev-parse --show-toplevel)"
cd "$ROOT"
# the macOS packages of the one workspace of the repository
PKGS=(-p komorebi -p komorebi-macos -p komorebic -p komorebi-client -p komorebi-bar)
export CARGO_TARGET_DIR="$S/target-fast-mac" COMMIT_HASH="${COMMIT_HASH:-dev}"
export CC_aarch64_apple_darwin="$HERE/cc-fake.sh" AR_aarch64_apple_darwin=ar
PKG_CONFIG_PATH="$(nix build --no-link --print-out-paths nixpkgs#openssl.dev)/lib/pkgconfig"
export PKG_CONFIG_PATH
exec nix shell nixpkgs#gcc nixpkgs#pkg-config nixpkgs#openssl.dev -c \
  cargo check --target aarch64-apple-darwin "${PKGS[@]}" --all-targets "$@"
