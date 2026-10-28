# The init of the guest that scripts/wsl2-probe.sh boots (nix/dev/wsl2-probe.nix puts the
# interpreter, PATH and `probe_loop` in front of this text). The kernel command line chooses what
# it does:
#
#   probe.mode=bench   runs the CPU loop, prints the CPU, and powers off (the default)
#   probe.mode=hold    prints a line every second for probe.hold seconds, then powers off: it
#                      keeps a window open long enough to be seen
#   probe.mode=virtiofs  loads the kernel's virtio and virtiofs modules, mounts the share that
#                      the host serves with virtiofsd (tag probe.fstag, default probe), reads
#                      probe.token's file and writes one, then powers off
#   probe.loops=N      the number of iterations of the loop (default 600000)
#   probe.hold=N       the number of seconds of the hold mode (default 8)
#   probe.serial=DEV   a serial device the lines are copied to, for a run whose console is the
#                      screen (default none: the console is the serial port)
#
# Every line that matters starts with `PROBE:` so that the host can find it in the console.

mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
exec </dev/console >/dev/console 2>&1

# param NAME DEFAULT: the value NAME has on the kernel command line
param() {
  value=$(tr ' ' '\n' </proc/cmdline | sed -n "s/^$1=//p" | tail -n 1)
  echo "${value:-$2}"
}

mode=$(param probe.mode bench)
hold=$(param probe.hold 8)
loops=$(param probe.loops 600000)
serial=$(param probe.serial none)
fstag=$(param probe.fstag probe)
token=$(param probe.token none)

# to the console, and to the serial port that the host reads when the console is the screen
say() {
  echo "PROBE:$*"
  [ -c "/dev/$serial" ] && echo "PROBE:$*" >"/dev/$serial"
}

say "START kernel=$(uname -r) machine=$(uname -m) mode=$mode"
say "CPU_MODEL=$(grep -m1 -E '^(model name|Processor|CPU part)' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
say "CPU_FLAGS=$(grep -m1 -E '^(flags|Features)' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
say "CPU_COUNT=$(grep -c '^processor' /proc/cpuinfo)"
say "MEM_KB=$(grep -m1 '^MemTotal' /proc/meminfo | tr -s ' ' | cut -d' ' -f2)"

if [ "$mode" = bench ]; then
  say "LOOP_ITERATIONS=$loops"
  say "LOOP_MS=$(probe_loop "$loops")"
fi

if [ "$mode" = virtiofs ]; then
  # virtio's PCI transport, FUSE and virtiofs are modules in this kernel, as they are in the real
  # guest's, whose initrd loads them before it mounts /nix/store
  for module in virtio_pci virtiofs; do
    if text=$(modprobe "$module" 2>&1); then
      say "MODULE_$module=loaded"
    elif [ "$module" = virtio_pci ] && [ -d /sys/bus/pci/drivers/virtio-pci ]; then
      # a kernel that has the transport built in (the aarch64 one) has no module of it
      say "MODULE_$module=built in"
    else
      say "MODULE_$module=failed: $text"
    fi
  done
  say "FILESYSTEMS=$(grep -E 'virtiofs|fuse' /proc/filesystems | tr -s '\t\n' '  ')"
  mkdir -p /store /share

  # the host's /nix/store, read-only: the guest's busybox is read through it and checksummed
  if text=$(mount -t virtiofs -o ro probestore /store 2>&1); then
    say "STORE_MOUNT=ok"
    say "STORE_SHA256=$(sha256sum "/store/$(param probe.storefile none)" 2>&1 | cut -d' ' -f1)"
    if echo x >/store/probe-write 2>/dev/null; then
      say "STORE_WRITE=allowed"
    else
      say "STORE_WRITE=refused"
    fi
  else
    say "STORE_MOUNT=failed: $text"
    say "DEVICES=$(cd /sys/bus/virtio/devices 2>&1 && echo *)"
  fi

  # a writable directory: the guest reads the file the host made and writes one of its own
  if text=$(mount -t virtiofs "$fstag" /share 2>&1); then
    say "SHARE_MOUNT=ok"
    say "SHARE_LISTING=$(cd /share && echo *)"
    say "SHARE_READ=$(cat /share/token 2>&1)"
    say "SHARE_READ_OWNER=$(stat -c '%u:%g %a' /share/token 2>&1)"
    if text=$({ echo "$token" >/share/from-guest; } 2>&1); then
      say "SHARE_WRITE=ok"
    else
      say "SHARE_WRITE=failed: $text"
    fi
    say "SHARE_WRITE_OWNER=$(stat -c '%u:%g %a' /share/from-guest 2>&1)"
  else
    say "SHARE_MOUNT=failed: $text"
  fi
  sync
fi

if [ "$mode" = hold ]; then
  n=0
  while [ "$n" -lt "$hold" ]; do
    say "ALIVE $n of $hold seconds: this window is the guest of wsl2-probe, it closes by itself"
    sleep 1
    n=$((n + 1))
  done
fi

say "DONE"
sync
poweroff -f
# a kernel that does not power off on request is stopped by the host's timeout
sleep 600
