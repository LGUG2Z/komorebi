#!/usr/bin/env bash
# W0: unattended Windows 11 evaluation install under QEMU + nested KVM.
# Prints a dated PASTE block with the wall-clock time of the install.
#
# usage: w0-install.sh ISO ANSWER_ISO WORKDIR
# needs on PATH: qemu-system-x86_64 qemu-img swtpm socat
# needs in env: OVMF_FD (the OVMFFull.fd output of nixpkgs)
set -euo pipefail

iso=$1 answer=$2 work=$3
: "${OVMF_FD:?set OVMF_FD to the OVMFFull.fd store path}"
: "${VCPUS:=4}" "${MEM:=6G}" "${DISK:=60G}"

rm -rf "$work/tpm"
mkdir -p "$work/tpm"
rm -f "$work/disk.qcow2" "$work/vars.fd" "$work/qmp.sock" "$work/tpm.sock"
qemu-img create -q -f qcow2 "$work/disk.qcow2" "$DISK"
cp "$OVMF_FD/FV/OVMF_VARS.fd" "$work/vars.fd"
chmod u+w "$work/vars.fd"

swtpm socket --tpm2 --tpmstate dir="$work/tpm" \
  --ctrl type=unixio,path="$work/tpm.sock" --daemon \
  --pid file="$work/swtpm.pid"

qmp() { # one command, no capabilities dance beyond the greeting
  printf '{"execute":"qmp_capabilities"}\n%s\n' "$1" |
    socat - "UNIX-CONNECT:$work/qmp.sock" >/dev/null
}
press() { # key tap through QMP
  qmp "{\"execute\":\"send-key\",\"arguments\":{\"keys\":[{\"type\":\"qcode\",\"data\":\"$1\"}]}}"
}

start=$(date +%s)
echo "install start: $(date -Is)"

qemu-system-x86_64 -name w0-install -accel kvm -machine q35,smm=off \
  -cpu host \
  -smp "$VCPUS" -m "$MEM" \
  -drive "if=pflash,format=raw,readonly=on,unit=0,file=$OVMF_FD/FV/OVMF_CODE.fd" \
  -drive "if=pflash,format=raw,unit=1,file=$work/vars.fd" \
  -chardev "socket,id=chrtpm,path=$work/tpm.sock" \
  -tpmdev emulator,id=tpm0,chardev=chrtpm \
  -device tpm-crb,tpmdev=tpm0 \
  -drive "if=none,id=disk,file=$work/disk.qcow2,format=qcow2,discard=unmap" \
  -device nvme,drive=disk,serial=e2e0,bootindex=2 \
  -drive "if=none,id=cd,file=$iso,media=cdrom,readonly=on" \
  -device ide-cd,drive=cd,bus=ide.0,bootindex=1 \
  -drive "if=none,id=ans,file=$answer,media=cdrom,readonly=on" \
  -device ide-cd,drive=ans,bus=ide.1 \
  -netdev user,id=net0 -device e1000e,netdev=net0 \
  -device qemu-xhci -device usb-tablet -device usb-kbd \
  -device VGA,vgamem_mb=64 \
  -display none -qmp "unix:$work/qmp.sock,server=on,wait=off" \
  &
qpid=$!

# "Press any key to boot from CD or DVD": tap space for the first minute.
# Later reboots must reach the disk, so stop after that.
for _ in $(seq 1 60); do
  sleep 1
  kill -0 "$qpid" 2>/dev/null || break
  [ -S "$work/qmp.sock" ] && press spc || true
done

wait "$qpid" || true
end=$(date +%s)
kill "$(cat "$work/swtpm.pid")" 2>/dev/null || true
echo "install end: $(date -Is)"
echo "PASTE install_seconds=$((end - start)) vcpus=$VCPUS mem=$MEM"
