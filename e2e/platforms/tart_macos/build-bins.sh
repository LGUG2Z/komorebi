#!/bin/sh
# Builds the programs the macOS guest runs and puts them in BIN_DIR, ready to upload:
#
#   build-bins.sh BIN_DIR [cargo args, e.g. --release]
#
# komorebi, komorebic, the test window and the agent, built for the host (the guest is the same
# architecture). A build in the dev shell of the flake links the libiconv of the nix store, which the
# guest does not have, so the library is pointed at the system's, as the nix package does, and the
# programs are signed again, as the system asks of a changed one.
set -eu

bin_dir=${1:?usage: build-bins.sh BIN_DIR [cargo args]}
shift
root=$(cd "$(dirname "$0")/../../.." && pwd)
cd "$root"

# the default dev shell builds for Windows: this is for the host
unset CARGO_BUILD_TARGET
profile=debug
case " $* " in *" --release "*) profile=release ;; esac

COMMIT_HASH=${COMMIT_HASH:-$(git rev-parse HEAD 2>/dev/null || echo dev)} \
  cargo build --locked -p komorebi -p komorebic -p komorebi-testwin -p komorebi-e2e-agent "$@"

mkdir -p "$bin_dir"
for bin in komorebi komorebic komorebi-testwin komorebi-e2e-agent; do
  rm -f "$bin_dir/$bin"
  cp "target/$profile/$bin" "$bin_dir/$bin"
  chmod u+w "$bin_dir/$bin"
  for lib in $(otool -L "$bin_dir/$bin" | grep -o '/nix/store/[^ ]*' || true); do
    install_name_tool -change "$lib" "/usr/lib/$(basename "$lib" | sed 's/\.[0-9.]*dylib/.dylib/')" "$bin_dir/$bin"
  done
  codesign -f -s - "$bin_dir/$bin" 2>/dev/null
done
git rev-parse HEAD > "$bin_dir/commit" 2>/dev/null || true
echo "build-bins: $bin_dir"
