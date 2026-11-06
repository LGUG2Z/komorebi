#!/bin/sh
# Builds the golden image of the macOS guest from a vanilla Tart image of Cirrus Labs.
#
#   build-golden.sh BIN_DIR [GOLDEN [VANILLA]]
#
# BIN_DIR is where `build-bins.sh` put komorebi-e2e-agent. GOLDEN is the name of the image to make
# (komorebi-e2e-golden), VANILLA the image to start from (the latest macOS of Cirrus Labs).
#
# One step cannot be scripted, and is the reason for a golden image: macOS asks a person to allow
# Screen Recording and Accessibility, once, for the program the agent runs under (Terminal). The
# grants live in the image and survive every clone of it. The script stops for it.
#
# It needs `tart` (KOMOREBI_E2E_TART or the path) and `sshpass` (the vanilla images log in with
# admin/admin), and about 30 GB of disk.
set -eu

bin_dir=${1:?usage: build-golden.sh BIN_DIR [GOLDEN [VANILLA]]}
golden=${2:-komorebi-e2e-golden}
vanilla=${3:-ghcr.io/cirruslabs/macos-tahoe-vanilla:latest}
tart=${KOMOREBI_E2E_TART:-tart}
here=$(cd "$(dirname "$0")" && pwd)

if "$tart" list | grep -q " $golden "; then
  echo "build-golden: $golden exists; delete it first (tart delete $golden)" >&2
  exit 1
fi

"$tart" clone "$vanilla" "$golden"
# the display of every guest is this size, which the scenarios and the work area rely on
"$tart" set "$golden" --display 1280x800 --cpu 4 --memory 8192

# a window, because a person has to use it
"$tart" run "$golden" > "${TMPDIR:-/tmp}/$golden-run.log" 2>&1 &
run=$!
ip=$("$tart" ip "$golden" --wait 120)
echo "build-golden: $golden is at $ip"

# a throwaway guest: its key is new every time, and the password is the one of the vanilla image
guest() {
  sshpass -p admin ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR "admin@$ip" "$@"
}
copy_to_guest() {
  sshpass -p admin scp -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
    -o LogLevel=ERROR "$@"
}
i=0
until guest true 2>/dev/null; do
  i=$((i + 1)); [ "$i" -lt 60 ] || { echo "build-golden: no ssh" >&2; exit 1; }
  sleep 2
done

guest 'mkdir -p ~/e2e/bin'
copy_to_guest "$here/provision.sh" "$bin_dir/komorebi-e2e-agent" "admin@$ip:/Users/admin/e2e/bin/"
guest 'sh ~/e2e/bin/provision.sh && rm ~/e2e/bin/provision.sh && chmod +x ~/e2e/bin/komorebi-e2e-agent'

cat <<'MSG'

build-golden: now, in the window of the guest:
  1. open Terminal and run:  ~/e2e/agent.command
  2. allow Screen Recording and Accessibility for Terminal when asked
     (System Settings, Privacy & Security; the password of the account is admin)
  3. when the agent runs without the prompts coming back, press Enter here.
MSG
read -r _

# the agent has to answer, or what was granted is not what a clone will run
python3 - "$ip" <<'PY'
import json, socket, sys
s = socket.create_connection((sys.argv[1], 7878), timeout=10)
f = s.makefile("rwb")
f.write(b'{"id": 1, "verb": "displays"}\n'); f.flush()
print("build-golden: the agent answers:", json.loads(f.readline()).get("ok") is not None)
PY

# a clean shutdown from inside, so that everything written is on the disk the clones copy: a
# `tart stop` of a running guest loses the last writes
guest 'sync; echo admin | sudo -S shutdown -h now' || true
wait "$run" || true
echo "build-golden: done; $golden is stopped and ready"
