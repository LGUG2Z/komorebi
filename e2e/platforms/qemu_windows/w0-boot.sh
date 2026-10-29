#!/usr/bin/env bash
# W0: boot the golden image on a throwaway overlay and time it to a desktop.
# A screendump every 5 s goes to WORKDIR/boot/; a PNG of a desktop is much
# larger than one of a spinner on black, which is the (crude) signal.
#
# usage: w0-boot.sh WORKDIR [SECONDS]     (WORKDIR has the golden disk.qcow2)
# needs on PATH: qemu-system-x86_64 qemu-img swtpm socat
# needs in env: OVMF_FD (an OVMF build with tpmSupport, without SMM)
set -euo pipefail

work=$1 limit=${2:-300}
: "${OVMF_FD:?set OVMF_FD to an OVMF fd store path with TPM support}"
: "${VCPUS:=4}" "${MEM:=6G}"

rm -rf "$work/boot" "$work/overlay.qcow2" "$work/qmp.sock" "$work/tpm.sock"
mkdir -p "$work/boot"
qemu-img create -q -f qcow2 -b "$work/disk.qcow2" -F qcow2 \
  "$work/overlay.qcow2"
cp "$work/vars.fd" "$work/vars-boot.fd"

swtpm socket --tpm2 --tpmstate dir="$work/tpm" \
  --ctrl type=unixio,path="$work/tpm.sock" --daemon \
  --pid file="$work/swtpm.pid"

start=$(date +%s)
echo "boot start: $(date -Is)"
qemu-system-x86_64 -name w0-boot -accel kvm -machine q35,smm=off \
  -cpu host -smp "$VCPUS" -m "$MEM" \
  -drive "if=pflash,format=raw,readonly=on,unit=0,file=$OVMF_FD/FV/OVMF_CODE.fd" \
  -drive "if=pflash,format=raw,unit=1,file=$work/vars-boot.fd" \
  -chardev "socket,id=chrtpm,path=$work/tpm.sock" \
  -tpmdev emulator,id=tpm0,chardev=chrtpm -device tpm-crb,tpmdev=tpm0 \
  -drive "if=none,id=disk,file=$work/overlay.qcow2,format=qcow2" \
  -device nvme,drive=disk,serial=e2e0,bootindex=1 \
  -netdev user,id=net0,hostfwd=tcp:127.0.0.1:7878-:7878 \
  -device e1000e,netdev=net0 \
  -device qemu-xhci -device usb-tablet -device usb-kbd \
  -device VGA,vgamem_mb=64 \
  ${QEMU_EXTRA:-} \
  -display none -qmp "unix:$work/qmp.sock,server=on,wait=off" &
qpid=$!

while kill -0 "$qpid" 2>/dev/null; do
  t=$(($(date +%s) - start))
  [ "$t" -ge "$limit" ] && break
  if [ -S "$work/qmp.sock" ]; then
    f="$work/boot/$(printf '%04d' "$t").png"
    printf '{"execute":"qmp_capabilities"}\n{"execute":"screendump","arguments":{"filename":"%s","format":"png"}}\n' "$f" |
      socat - "UNIX-CONNECT:$work/qmp.sock" >/dev/null || true
    [ -f "$f" ] && echo "t=$t png_bytes=$(stat -c %s "$f")"
  fi
  sleep 5
done
echo "boot end: $(date -Is) (left running; stop the unit to end it)"
wait "$qpid" || true
