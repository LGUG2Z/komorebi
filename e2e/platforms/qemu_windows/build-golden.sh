#!/usr/bin/env bash
# Builds the golden Windows image on top of a base from build-base.sh: boots
# an overlay of the base, provisions it through the guest agent (provision.ps1:
# policies, Defender, the display driver) and shuts it down, in about 2.5
# minutes. OUTDIR gets disk.qcow2, which is backed by the base through a
# relative path (so the two can move together), vars.fd and tpm/; the per-run
# overlays are backed by that.
#
# usage: build-golden.sh BASE_DIR OUTDIR QXLDOD_DIR
# QXLDOD_DIR is virtio-win's qxldod/w10/amd64, the display driver that gives a
# second QXL device a monitor.
# needs on PATH: qemu-system-x86_64 qemu-img swtpm
# needs in env: OVMF_FD (an OVMF build with tpmSupport, without SMM)
# run from e2e/ with PYTHONPATH=lib:. (the nix app does)
set -euo pipefail

exec python -m platforms.qemu_windows.build_golden "$@"
