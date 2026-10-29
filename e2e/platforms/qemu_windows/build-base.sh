#!/usr/bin/env bash
# Builds the BASE of the golden Windows image: the unattended install of
# w0-install.sh up to a desktop with auto-login, the Visual C++ runtime and the
# guest agent in the Run key, and nothing else. That is almost all of the time
# of a build (about 18 minutes of 20), and it rarely changes, so it is built
# once; build-golden.sh puts the rest on top of it in about 2.5 minutes.
#
# Rebuild it for a new ISO, when the agent needs something more in the image,
# or when the 90-day evaluation runs out.
#
# usage: build-base.sh ISO AGENT_EXE OUTDIR
# needs on PATH: xorriso qemu-system-x86_64 qemu-img swtpm socat
# needs in env: OVMF_FD (an OVMF build with tpmSupport, without SMM)
set -euo pipefail

iso=$1 agent=$2 out=$3
here=$(cd "$(dirname "$0")" && pwd)

mkdir -p "$out"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# autounattend.xml runs `setup.ps1` from the first drive that has one
cp "$here/autounattend.xml" "$agent" "$stage/"
cp "$here/setup-base.ps1" "$stage/setup.ps1"
xorriso -as mkisofs -J -r -V ANSWER -o "$out/answer.iso" "$stage"

"$here/w0-install.sh" "$iso" "$out/answer.iso" "$out"

# the installed disk is the base of the golden image: leave it alone
chmod a-w "$out/disk.qcow2"
echo "base image: $out/disk.qcow2 ($(date -Is))"
