#!/usr/bin/env bash
# wsl2-probe: finds out what a NixOS-WSL2 virtual machine can do for a Plasma guest.
# Start it as `nix run .#wsl2-probe` (or `just wsl2-probe`) and paste the end of its output
# back. `--help` lists the options, docs/linux/dev/wsl2-probe.md says what each check means.
set -u
export LC_ALL=C
# wsl.exe writes UTF-16 unless it is told otherwise; WSLENV carries the variable to it
export WSL_UTF8=1
export WSLENV="${WSLENV:+$WSLENV:}WSL_UTF8"

usage() {
  cat <<'EOF'
wsl2-probe: finds out what a NixOS-WSL2 virtual machine can do for a Plasma guest.

  nix run .#wsl2-probe [-- OPTIONS]        (or: just wsl2-probe)

It changes nothing on the machine. It reads files, boots a tiny guest (a kernel and an initrd
that print, time a loop and power off) under QEMU, and listens on local ports for a few seconds.
It prints PASS, WARN and FAIL lines, with the exact fix for the last two, writes the same
report to ./wsl2-probe-report.txt, and ends with a block to paste back.

  P0  the environment: WSL2, kernel, memory, disk, .wslconfig, the CPU, Nix
  P1  KVM: /dev/kvm, who can open it, a guest booted under KVM and under TCG, the speeds
  P2  display: WSLg, and a QEMU window that stays open for a few seconds
  P3  GPU: /dev/dxg, the Windows driver, the host's OpenGL renderer, a VirGL window
  P4  the Windows side: interop, mstsc.exe, whether Windows reaches a port listening in WSL
  P5  resources: whether memory, cores and disk are enough for the real guest
  P6  virtiofs: /dev/fuse, virtiofsd, and a guest that mounts what virtiofsd serves

Options:
  --only P1,P3     run only these probes (P0 always runs)
  --hold SECONDS   how long a window stays open (default 8)
  --yes            answer yes to "did a window appear?" without asking
  --no-prompt      do not ask anything (also the case when stdin is not a terminal)
  --dry-run        run P0 and P5 and print what the other probes would start, without starting it
  --report FILE    where to write the report (default ./wsl2-probe-report.txt)
  -h, --help       this text

The exit status is 0 when the probes ran, whatever they found; 2 for a mistake in the options.
EOF
}

DRY=0
ASSUME_YES=0
NO_PROMPT=0
HOLD=8
LOOPS=600000
ONLY=""
REPORT=wsl2-probe-report.txt
GUEST=${WSL2_PROBE_GUEST:-}
REV=${WSL2_PROBE_REV:-unknown}

need_value() {
  if [ "$1" -lt 2 ]; then
    echo "$2 needs a value (see --help)" >&2
    exit 2
  fi
}

while [ $# -gt 0 ]; do
  case $1 in
  --only)
    need_value $# --only
    ONLY=${2^^}
    shift
    ;;
  --hold)
    need_value $# --hold
    HOLD=$2
    shift
    ;;
  --report)
    need_value $# --report
    REPORT=$2
    shift
    ;;
  --yes) ASSUME_YES=1 ;;
  --no-prompt) NO_PROMPT=1 ;;
  --dry-run) DRY=1 ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "unknown option: $1 (see --help)" >&2
    exit 2
    ;;
  esac
  shift
done
case $HOLD in '' | *[!0-9]*)
  echo "--hold needs a whole number of seconds" >&2
  exit 2
  ;;
esac

STATE=$(mktemp -d "${TMPDIR:-/tmp}/wsl2-probe.XXXXXX") || {
  echo "cannot make a temporary directory" >&2
  exit 2
}
mkdir -p "$STATE/kv"
: >"$STATE/verdicts"
: >"$STATE/facts"
: >"$STATE/pids"

# only what this script started is stopped: the listeners of P4 (the trap below calls it)
# shellcheck disable=SC2329
cleanup() {
  local pid
  while read -r pid; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null
  done <"$STATE/pids"
  rm -rf "$STATE"
}
trap 'cleanup' EXIT
trap 'echo; echo "interrupted: the report so far is in $REPORT"; exit 130' INT TERM

if ! : >"$REPORT" 2>/dev/null; then
  REPORT=${TMPDIR:-/tmp}/wsl2-probe-report.txt
  : >"$REPORT"
fi
case $REPORT in
/*) REPORT_PATH=$REPORT ;;
*) REPORT_PATH=$PWD/$REPORT ;;
esac

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  C_PASS=$'\033[32m'
  C_WARN=$'\033[33m'
  C_FAIL=$'\033[31m'
  C_INFO=$'\033[36m'
  C_BOLD=$'\033[1m'
  C_OFF=$'\033[0m'
else
  C_PASS="" C_WARN="" C_FAIL="" C_INFO="" C_BOLD="" C_OFF=""
fi

# ---------------------------------------------------------------------------------------------
# output: every line goes to the screen and to the report
# ---------------------------------------------------------------------------------------------

out() {
  printf '%s\n' "$*"
  printf '%s\n' "$*" >>"$REPORT"
}

# a status line: [PASS] text
line() {
  local status=$1 color
  shift
  case $status in
  PASS) color=$C_PASS ;;
  WARN) color=$C_WARN ;;
  FAIL) color=$C_FAIL ;;
  *) color=$C_INFO ;;
  esac
  printf '[%s%s%s] %s\n' "$color" "$status" "$C_OFF" "$*"
  printf '[%s] %s\n' "$status" "$*" >>"$REPORT"
}

# verdict ID STATUS TEXT: a check with a result, which the summary lists
verdict() {
  local id=$1 status=$2
  shift 2
  line "$status" "$id $*"
  printf '%s|%s|%s\n' "$id" "$status" "$*" >>"$STATE/verdicts"
}

# fix LINE...: how to repair the check above
fix() {
  local first=1 text
  for text in "$@"; do
    if [ "$first" = 1 ]; then
      out "       fix: $text"
      first=0
    else
      out "            $text"
    fi
  done
}

# detail LINE...: more about the check above
detail() {
  local text
  for text in "$@"; do
    out "       $text"
  done
}

# step TEXT: says what is about to take a while
step() { out "       ... $*"; }

# rc_text STATUS: how a run ended, for a message
rc_text() {
  if [ "$1" = 124 ]; then echo "the time limit stopped it"; else echo "exit status $1"; fi
}

# block PREFIX: prints stdin, with the prefix, to the screen and the report
block() {
  local text
  while IFS= read -r text; do
    out "$1$text"
  done
}

# fact PROBE KEY VALUE: a value read or measured, which the summary repeats
fact() {
  local probe=$1 key=$2 value
  shift 2
  value=$(printf '%s' "$*" | tr '\n' ' ')
  line INFO "$probe $key: $value"
  printf '%s: %s\n' "$key" "$value" >>"$STATE/facts"
}

# fact_block PROBE KEY: a fact of several lines, read from stdin
fact_block() {
  local text
  text=$(cat)
  line INFO "$1 $2:"
  printf '%s\n' "$text" | block "         | "
  {
    printf '%s:\n' "$2"
    printf '%s\n' "$text" | sed 's/^/  /'
  } >>"$STATE/facts"
}

heading() {
  out ""
  printf '%s== %s%s\n' "$C_BOLD" "$*" "$C_OFF"
  printf '== %s\n' "$*" >>"$REPORT"
}

kv_set() { printf '%s\n' "$2" >"$STATE/kv/$1"; }

kv_get() {
  if [ -f "$STATE/kv/$1" ]; then cat "$STATE/kv/$1"; else printf '%s\n' "${2:-}"; fi
}

want() {
  [ -z "$ONLY" ] && return 0
  case ",$ONLY," in *",$1,"*) return 0 ;; esac
  return 1
}

have() { command -v "$1" >/dev/null 2>&1; }

now_ms() {
  local t=${EPOCHREALTIME/[.,]/}
  echo $((t / 1000))
}

gib() { awk -v k="${1:-0}" 'BEGIN { printf "%.1f", k / 1048576 }'; }

# list_dir DIR: the names in a directory, one per line; nothing when there is none
list_dir() { find "$1" -mindepth 1 -maxdepth 1 -printf '%f\n' 2>/dev/null | sort; }

# file_owner FILE: the mode, owner and group of a file
file_owner() { stat -c '%A %U %G' "$1" 2>/dev/null; }

# head_lines COUNT FILE: the first non-empty lines of a file, for an error message
head_lines() { tr -d '\r' 2>/dev/null <"$2" | grep -v '^$' | head -n "$1"; }

# ---------------------------------------------------------------------------------------------
# the machine
# ---------------------------------------------------------------------------------------------

ME=$(id -un 2>/dev/null || echo "${USER:-user}")
DISTRO=${WSL_DISTRO_NAME:-NixOS}
STATE_DIR=${XDG_STATE_HOME:-$HOME/.local/state}/komorebi-run-kde

is_wsl() {
  [ -n "${WSL_DISTRO_NAME:-}" ] || [ -d /run/WSL ] || grep -qi microsoft /proc/version 2>/dev/null
}

# a Windows program: on PATH (NixOS-WSL puts the Windows PATH there) or in System32
find_win() {
  local name=$1 dir found
  if found=$(command -v "$name" 2>/dev/null); then
    echo "$found"
    return 0
  fi
  for dir in /mnt/c/Windows/System32 /mnt/c/Windows /mnt/c/Windows/System32/WindowsPowerShell/v1.0; do
    if [ -x "$dir/$name" ]; then
      echo "$dir/$name"
      return 0
    fi
  done
  return 1
}

# win PROGRAM ARGS...: runs a Windows program from a Windows directory (cmd.exe complains about
# a Linux one) and prints its output without carriage returns, after WIN_TIMEOUT seconds at most
win() {
  local program=$1 path
  shift
  [ "$DRY" = 1 ] && return 127
  path=$(find_win "$program") || return 127
  (
    cd /mnt/c 2>/dev/null || cd /
    timeout "${WIN_TIMEOUT:-20}" "$path" "$@" 2>&1 </dev/null | tr -d '\r\0'
  )
}

# the WSL path of a Windows path
win_to_wsl() {
  local path=$1 drive rest
  if have wslpath && wslpath -u "$path" 2>/dev/null; then
    return 0
  fi
  rest=${path//\\//}
  drive=${rest%%:*}
  echo "/mnt/${drive,,}${rest#*:}"
}

# the .wslconfig of the Windows user, as a WSL path; nothing when interop is off
wslconfig_path() {
  local cached profile
  cached=$(kv_get wslconfig_path)
  if [ -n "$cached" ]; then
    [ "$cached" = none ] || echo "$cached"
    return 0
  fi
  profile=$(win cmd.exe /c 'echo %USERPROFILE%' | grep -m1 -E '^[A-Za-z]:[\\]')
  if [ -z "$profile" ]; then
    kv_set wslconfig_path none
    return 0
  fi
  kv_set wslconfig_path "$(win_to_wsl "$profile")/.wslconfig"
  kv_set wslconfig_name "$profile\\.wslconfig"
  kv_get wslconfig_path
}

# the .wslconfig as Windows names it, for a fix
wslconfig_name() {
  local name
  wslconfig_path >/dev/null
  name=$(kv_get wslconfig_name)
  echo "${name:-%USERPROFILE%\\.wslconfig}"
}

# wslconfig_get KEY: the value a key has in .wslconfig, nothing when it is not set
wslconfig_get() {
  local file
  file=$(wslconfig_path)
  [ -n "$file" ] && [ -r "$file" ] || return 1
  tr -d '\r' <"$file" | awk -F= -v key="$1" '
    { name = $1; gsub(/^[ \t]+|[ \t]+$/, "", name) }
    tolower(name) == tolower(key) {
      value = substr($0, index($0, "=") + 1)
      sub(/[ \t]*[#;].*$/, "", value)
      gsub(/^[ \t]+|[ \t]+$/, "", value)
      print value
      exit
    }'
}

# the free space of the file system a path is on (or its nearest parent that exists)
df_avail_kib() {
  local path=$1
  while [ ! -e "$path" ] && [ "$path" != / ]; do path=$(dirname "$path"); done
  df -Pk "$path" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

df_fstype() {
  local path=$1
  while [ ! -e "$path" ] && [ "$path" != / ]; do path=$(dirname "$path"); done
  df -PkT "$path" 2>/dev/null | awk 'NR == 2 { print $2 }'
}

mem_kib() { awk -v key="$1:" '$1 == key { print $2 }' /proc/meminfo; }

# the free space the Nix store and the guest share: under WSL the virtual disk looks huge, and
# what limits it is the Windows drive it lives on
disk_free_kib() {
  local nix_free c_free
  nix_free=$(df_avail_kib /nix)
  if is_wsl && [ -d /mnt/c ]; then
    c_free=$(df_avail_kib /mnt/c)
    if [ -n "$c_free" ] && [ -n "$nix_free" ] && [ "$c_free" -lt "$nix_free" ]; then
      echo "$c_free"
      return
    fi
  fi
  echo "$nix_free"
}

cpu_flag() { grep -m1 -o -w -E 'svm|vmx' /proc/cpuinfo 2>/dev/null | head -n1; }

# ---------------------------------------------------------------------------------------------
# the guest: a kernel and an initrd that print, time a loop and power off
# ---------------------------------------------------------------------------------------------

case $(uname -m) in
x86_64)
  QEMU=qemu-system-x86_64
  MACHINE=q35
  SERIAL=ttyS0
  VGA=virtio-vga
  VGA_GL=virtio-vga-gl
  ;;
aarch64)
  QEMU=qemu-system-aarch64
  MACHINE=virt,gic-version=max
  SERIAL=ttyAMA0
  VGA=virtio-gpu-pci
  VGA_GL=virtio-gpu-gl-pci
  ;;
*)
  QEMU=qemu-system-$(uname -m)
  MACHINE=virt
  SERIAL=ttyS0
  VGA=virtio-vga
  VGA_GL=virtio-vga-gl
  ;;
esac

guest_ready() {
  [ -n "$GUEST" ] && [ -r "$GUEST/kernel" ] && [ -r "$GUEST/initrd" ] && have "$QEMU"
}

guest_missing() {
  verdict "$1" SKIP "no guest or no QEMU: they come with nix run .#wsl2-probe"
  if [ -z "$GUEST" ]; then
    fix "start the script as nix run .#wsl2-probe from the checkout (it brings the guest and QEMU)"
  else
    fix "$QEMU is not on PATH: start the script as nix run .#wsl2-probe"
  fi
}

# qemu_args MODE ACCEL... -- EXTRA...: fills QARGS with the guest's command line. MODE is the
# guest's probe.mode; ACCEL are the accelerators to try in order (kvm, tcg).
qemu_args() {
  local mode=$1 cpu=host append
  shift
  QARGS=("$QEMU" -machine "$MACHINE")
  while [ $# -gt 0 ] && [ "$1" != -- ]; do
    QARGS+=(-accel "$1")
    # a run that may fall back to software cannot ask for the host's CPU
    [ "$1" = tcg ] && cpu=max
    shift
  done
  shift
  append="panic=-1 quiet probe.mode=$mode probe.hold=$HOLD probe.loops=$LOOPS"
  case $mode in
  bench) append="console=$SERIAL $append" ;;
  virtiofs) append="console=$SERIAL $append probe.token=${P6_TOKEN:-none} probe.storefile=${P6_STOREFILE:-none}" ;;
  *) append="console=$SERIAL console=tty0 probe.serial=$SERIAL $append" ;;
  esac
  QARGS+=(-cpu "$cpu" -smp 1 -m 512 -no-reboot -nodefaults
    -kernel "$GUEST/kernel" -initrd "$GUEST/initrd" -append "$append"
    -serial "file:$STATE/serial" "$@")
}

# run_guest LIMIT ENV... -- COMMAND...: runs the command for at most LIMIT seconds and sets
# RG_RC (its exit status, 124 when the limit stopped it), RG_MS (the wall time), RG_SERIAL
# (what the guest printed, without carriage returns) and RG_ERR (what QEMU printed)
run_guest() {
  local limit=$1 start
  local -a venv=()
  shift
  while [ $# -gt 0 ] && [ "$1" != -- ]; do
    venv+=("$1")
    shift
  done
  shift
  : >"$STATE/serial"
  RG_ERR=$STATE/qemu.err
  RG_SERIAL=$STATE/serial.clean
  start=$(now_ms)
  env "${venv[@]}" timeout "$limit" "$@" >"$STATE/qemu.out" 2>"$RG_ERR" </dev/null
  RG_RC=$?
  RG_MS=$(($(now_ms) - start))
  tr -d '\r' <"$STATE/serial" >"$RG_SERIAL"
  cat "$STATE/qemu.out" >>"$RG_ERR"
}

# guest_value FILE KEY: a PROBE:KEY=value line the guest printed
guest_value() { sed -n "s/^PROBE:$2=//p" "$1" | head -n1; }

# ask_yes_no QUESTION: prints yes, no or "not asked"
ask_yes_no() {
  local answer=""
  if [ "$ASSUME_YES" = 1 ]; then
    echo "yes (answered by --yes)"
    return
  fi
  if [ "$NO_PROMPT" = 1 ] || [ ! -t 0 ]; then
    echo "not asked"
    return
  fi
  printf '%s (y/n) ' "$1" >&2
  if read -r -t 120 answer; then
    case $answer in
    [yY]*) echo yes ;;
    [nN]*) echo no ;;
    *) echo "not understood: $answer" ;;
    esac
  else
    echo "no answer"
  fi
}

# ---------------------------------------------------------------------------------------------
# P0: the environment
# ---------------------------------------------------------------------------------------------

p0_environment() {
  heading "P0 environment"
  local text path vendor model ver wslv config

  if is_wsl; then
    if grep -qi 'wsl2' /proc/version 2>/dev/null; then
      verdict P0.1 PASS "this is WSL2 (distribution $DISTRO)"
    else
      verdict P0.1 WARN "this is WSL, but /proc/version does not say WSL2: $(head -n1 /proc/version)"
      fix "in PowerShell: wsl --set-version $DISTRO 2"
    fi
  else
    verdict P0.1 WARN "this is not WSL: the Windows side (P4) and WSLg (P2, P3) can only report what they find here"
  fi
  fact P0 wsl "WSL_DISTRO_NAME=${WSL_DISTRO_NAME:-unset}; /run/WSL $([ -d /run/WSL ] && echo present || echo absent); WSL_INTEROP=${WSL_INTEROP:-unset}"
  fact P0 kernel "$(uname -srm)"
  fact P0 proc_version "$(head -n1 /proc/version)"
  fact P0 pid1 "$(ps -p 1 -o comm= 2>/dev/null || echo unknown)"
  if [ -r /etc/os-release ]; then
    text=$(
      # shellcheck source=/dev/null
      . /etc/os-release
      echo "${PRETTY_NAME:-?} (VERSION_ID ${VERSION_ID:-?}, BUILD_ID ${BUILD_ID:-?})"
    )
    fact P0 os "$text"
  fi
  have nixos-version && fact P0 nixos_version "$(nixos-version 2>/dev/null)"
  if [ -r /etc/nixos/flake.lock ] && have jq; then
    text=$(jq -r '.nodes["nixos-wsl"].locked // empty | "\(.owner)/\(.repo) \(.rev) lastModified \(.lastModified)"' /etc/nixos/flake.lock 2>/dev/null)
    [ -n "$text" ] && fact P0 nixos_wsl_input "$text"
  fi
  fact P0 wsl_commands "$(list_dir /run/current-system/sw/bin | grep -i wsl | tr '\n' ' ')"

  fact P0 memory "MemTotal $(gib "$(mem_kib MemTotal)") GiB, MemAvailable $(gib "$(mem_kib MemAvailable)") GiB, SwapTotal $(gib "$(mem_kib SwapTotal)") GiB"
  fact P0 cpus "$(nproc) (nproc)"
  for path in /nix "$HOME" "$STATE_DIR" "$PWD" /mnt/c; do
    [ -e "$path" ] || [ "$path" = "$STATE_DIR" ] || continue
    fact P0 "free_disk $path" "$(gib "$(df_avail_kib "$path")") GiB ($(df_fstype "$path"))"
  done

  vendor=$(awk -F: '/^vendor_id/ { gsub(/^ /, "", $2); print $2; exit }' /proc/cpuinfo)
  model=$(awk -F: '/^model name/ { gsub(/^ /, "", $2); print $2; exit }' /proc/cpuinfo)
  fact P0 cpu "${vendor:-unknown}, ${model:-unknown}; virtualization flag: $(cpu_flag)"

  ver=$(win cmd.exe /c ver | grep -m1 Windows)
  if [ -n "$ver" ]; then
    fact P0 windows "$ver"
    wslv=$(win wsl.exe --version | tr '\n' ';')
    [ -n "$wslv" ] && fact P0 wsl_exe_version "$wslv"
    config=$(wslconfig_path)
    if [ -r "$config" ]; then
      fact P0 wslconfig_path "$config"
      tr -d '\r' <"$config" | grep -v '^[#;]' | grep -v '^$' | head -n 40 | fact_block P0 wslconfig
    else
      fact P0 wslconfig "none at ${config:-?}: the defaults apply (half of the Windows memory, every processor)"
    fi
  elif [ "$DRY" = 1 ]; then
    fact P0 windows "dry run: the Windows side is not asked"
  elif is_wsl; then
    fact P0 windows "interop is off, or cmd.exe cannot be found: the Windows version and .wslconfig are unknown (P4.1 has the fix)"
  else
    fact P0 windows "not WSL: there is no Windows side"
  fi

  p0_nix
}

p0_nix() {
  local features trusted entry is_trusted=no
  if ! have nix; then
    verdict P0.2 FAIL "nix is not on PATH"
    fix "use a NixOS-WSL distribution, or install Nix; this script is meant for nix run .#wsl2-probe"
    return
  fi
  fact P0 nix "$(nix --version 2>/dev/null)"
  features=$(nix config show experimental-features 2>/dev/null || nix show-config 2>/dev/null | sed -n 's/^experimental-features = //p')
  case " $features " in
  *" flakes "*) verdict P0.2 PASS "flakes are enabled (experimental-features = $features)" ;;
  *)
    verdict P0.2 WARN "flakes do not show as enabled (experimental-features = ${features:-empty})"
    fix "NixOS: nix.settings.experimental-features = [ \"nix-command\" \"flakes\" ];  then sudo nixos-rebuild switch" \
      "for one command: nix --extra-experimental-features 'nix-command flakes' run .#wsl2-probe"
    ;;
  esac
  trusted=$(nix config show trusted-users 2>/dev/null || nix show-config 2>/dev/null | sed -n 's/^trusted-users = //p')
  fact P0 nix_trusted_users "${trusted:-unknown}; this user $ME, groups $(id -nG | tr ' ' ',')"
  for entry in $trusted; do
    case $entry in
    "$ME") is_trusted=yes ;;
    @*) id -nG | tr ' ' '\n' | grep -qx "${entry#@}" && is_trusted=yes ;;
    esac
  done
  if [ "$is_trusted" = yes ]; then
    verdict P0.3 PASS "$ME is a trusted nix user"
  else
    verdict P0.3 WARN "$ME is not a trusted nix user: nix run works, but a build cannot set options such as substituters"
    fix "NixOS-WSL: nix.settings.trusted-users = [ \"root\" \"$ME\" ];  then sudo nixos-rebuild switch"
  fi
  fact P0 nix_substituters "$(nix config show substituters 2>/dev/null || nix show-config 2>/dev/null | sed -n 's/^substituters = //p')"
}

# ---------------------------------------------------------------------------------------------
# P1: KVM
# ---------------------------------------------------------------------------------------------

p1_kvm() {
  heading "P1 KVM"
  local flag param nested=""
  flag=$(cpu_flag)

  if [ -n "$flag" ]; then
    verdict P1.1 PASS "the CPU exposes $flag to this machine"
  elif [ "$(uname -m)" != x86_64 ]; then
    verdict P1.1 INFO "this is $(uname -m), which has no svm or vmx flag: /dev/kvm (P1.2) says whether KVM works"
  else
    verdict P1.1 FAIL "the CPU flags have neither svm (AMD) nor vmx (Intel): no virtualization reaches this machine"
    if is_wsl; then
      fix "in $(wslconfig_name):" \
        "  [wsl2]" \
        "  nestedVirtualization=true" \
        "then in PowerShell: wsl --shutdown, and reopen the distribution" \
        "if it stays off: turn on SVM Mode (AMD) or Intel VT-x in the firmware setup, turn on the Windows" \
        "feature Virtual Machine Platform, and run wsl --update (nested virtualization needs Windows 11)"
    else
      fix "turn on SVM Mode (AMD) or Intel VT-x in the firmware setup"
    fi
  fi

  if [ ! -e /dev/kvm ]; then
    verdict P1.2 FAIL "/dev/kvm does not exist"
    if is_wsl; then
      fix "WSL2 creates it only when nested virtualization works: see P1.1; .wslconfig [wsl2] must not have" \
        "nestedVirtualization=false; then wsl --shutdown, and reopen"
    elif [ "$(uname -m)" = x86_64 ]; then
      fix "sudo modprobe kvm_amd   (or kvm_intel), and turn on virtualization in the firmware setup"
    else
      fix "this machine offers no KVM (a virtual machine without nested virtualization, or a kernel without it)"
    fi
    kv_set kvm_usable no
  elif (exec 9<>/dev/kvm) 2>/dev/null; then
    verdict P1.2 PASS "/dev/kvm can be opened read-write by $ME ($(file_owner /dev/kvm))"
    kv_set kvm_usable yes
  else
    verdict P1.2 FAIL "/dev/kvm exists, but $ME cannot open it read-write ($(file_owner /dev/kvm); groups: $(id -nG))"
    fix "put $ME in the group kvm, in the NixOS-WSL configuration:" \
      "  users.users.$ME.extraGroups = [ \"kvm\" ];" \
      "then: sudo nixos-rebuild switch" \
      "then in PowerShell: wsl --terminate $DISTRO   (groups are read at login), and reopen"
    kv_set kvm_usable no
  fi
  for param in /sys/module/kvm_amd/parameters/nested /sys/module/kvm_intel/parameters/nested; do
    [ -r "$param" ] && nested="$param = $(cat "$param")"
  done
  fact P1 nested_virtualization "${nested:-no kvm_amd or kvm_intel module parameter to read}"

  if [ "$DRY" = 1 ]; then
    dry_guest_plan
    return
  fi
  if ! guest_ready; then
    guest_missing P1.3
    return
  fi
  fact P1 qemu "$("$QEMU" --version | head -n1)"
  p1_boots
}

p1_boots() {
  local host_ms kvm_ms="" tcg_ms="" flags item seen=""
  # shellcheck disable=SC2016
  host_ms=$("$GUEST/busybox" sh -c '. "$1"; probe_loop "$2"' sh "$GUEST/loop.sh" "$LOOPS" 2>/dev/null)
  fact P1 loop_on_this_machine_ms "${host_ms:-unknown} for $LOOPS iterations of the guest's ash loop (the guest's busybox)"

  if [ "$(kv_get kvm_usable)" = yes ]; then
    step "booting the guest under KVM"
    qemu_args bench kvm -- -display none
    run_guest 90 -- "${QARGS[@]}"
    kvm_ms=$(guest_value "$RG_SERIAL" LOOP_MS)
    if [ "$RG_RC" = 0 ] && [ -n "$kvm_ms" ]; then
      verdict P1.3 PASS "a guest booted under KVM and ran the loop in $kvm_ms ms (this machine: ${host_ms:-?} ms); boot to power-off $RG_MS ms"
      flags=$(guest_value "$RG_SERIAL" CPU_FLAGS)
      fact P1 guest_cpu "$(guest_value "$RG_SERIAL" CPU_MODEL), $(guest_value "$RG_SERIAL" CPU_COUNT) cpu, $(guest_value "$RG_SERIAL" MEM_KB) kB"
      for item in svm vmx hypervisor avx avx2 sse4_2 aes; do
        case " $flags " in *" $item "*) seen="$seen$item " ;; esac
      done
      fact P1 guest_flags_of_interest "${seen:-none}"
    else
      verdict P1.3 FAIL "the guest did not run under KVM ($(rc_text "$RG_RC"))"
      head_lines 6 "$RG_ERR" | block "       qemu: "
      qemu_error_hint
      kvm_ms=""
    fi
  else
    verdict P1.3 SKIP "KVM is not usable (P1.1, P1.2)"
  fi

  step "booting the guest under TCG (software emulation: this takes longer)"
  qemu_args bench tcg -- -display none
  run_guest 300 -- "${QARGS[@]}"
  tcg_ms=$(guest_value "$RG_SERIAL" LOOP_MS)
  if [ "$RG_RC" = 0 ] && [ -n "$tcg_ms" ]; then
    verdict P1.4 PASS "a guest booted under TCG (software emulation) and ran the loop in $tcg_ms ms; boot to power-off $RG_MS ms"
  else
    verdict P1.4 FAIL "the guest did not run under TCG ($(rc_text "$RG_RC")): QEMU itself does not work here"
    head_lines 6 "$RG_ERR" | block "       qemu: "
    tcg_ms=""
  fi

  if [ -n "$kvm_ms" ] && [ -n "$tcg_ms" ]; then
    local ratio
    ratio=$(awk -v t="$tcg_ms" -v k="$kvm_ms" 'BEGIN { if (k < 1) k = 1; printf "%.1f", t / k }')
    fact P1 kvm_vs_tcg "TCG $tcg_ms ms / KVM $kvm_ms ms = ${ratio}x"
    if awk -v r="$ratio" 'BEGIN { exit !(r >= 3) }'; then
      verdict P1.5 PASS "KVM is ${ratio}x faster than TCG: several times, as hardware acceleration should be"
    elif awk -v r="$ratio" 'BEGIN { exit !(r >= 1.5) }'; then
      verdict P1.5 WARN "KVM is only ${ratio}x faster than TCG: it works, but less than it should"
      fix "run the probe again with nothing else busy on Windows; if it stays under 3x, say so when you paste this"
    else
      verdict P1.5 FAIL "KVM is ${ratio}x TCG: the guest ran, but not accelerated"
      fix "see P1.1 to P1.3, and say so when you paste this"
    fi
  else
    verdict P1.5 SKIP "no KVM against TCG ratio: one of the two runs is missing"
  fi
}

qemu_error_hint() {
  if grep -qi 'permission denied' "$RG_ERR" 2>/dev/null; then
    fix "QEMU was refused /dev/kvm: see P1.2 (the group kvm)"
  elif grep -qiE 'no such file|could not access kvm' "$RG_ERR" 2>/dev/null; then
    fix "there is no usable /dev/kvm: see P1.1 and P1.2"
  elif grep -qiE 'not supported|entry failed' "$RG_ERR" 2>/dev/null; then
    fix "KVM exists, but a CPU feature it needs is not passed through: .wslconfig [wsl2] nestedVirtualization=true," \
      "then wsl --shutdown; and SVM Mode (AMD) or VT-x (Intel) on in the firmware setup"
  else
    fix "QEMU could not start KVM: see P1.1 and P1.2, and say what QEMU printed when you paste this"
  fi
}

# ---------------------------------------------------------------------------------------------
# P2: display
# ---------------------------------------------------------------------------------------------

# resolve_display: sets DISPLAY_ENV (variables to add to the shell's), HAVE_WAYLAND, HAVE_X11 and
# DISPLAY_NOTE; fails when there is no display at all
resolve_display() {
  local runtime=${XDG_RUNTIME_DIR:-} wayland=${WAYLAND_DISPLAY:-} x11=${DISPLAY:-}
  DISPLAY_ENV=()
  DISPLAY_NOTE=""
  HAVE_WAYLAND=no
  HAVE_X11=no
  if [ -n "$wayland" ] && [ -S "${runtime:-/nonexistent}/$wayland" ]; then
    HAVE_WAYLAND=yes
    DISPLAY_NOTE="the shell's Wayland socket $runtime/$wayland"
  elif [ -S /mnt/wslg/runtime-dir/wayland-0 ]; then
    HAVE_WAYLAND=yes
    DISPLAY_ENV+=("XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir" "WAYLAND_DISPLAY=wayland-0")
    DISPLAY_NOTE="WSLg's Wayland socket /mnt/wslg/runtime-dir/wayland-0 (the shell does not export it)"
  fi
  if [ -n "$x11" ]; then
    HAVE_X11=yes
    DISPLAY_NOTE="${DISPLAY_NOTE:+$DISPLAY_NOTE, and }the shell's DISPLAY=$x11"
  elif [ -S /tmp/.X11-unix/X0 ]; then
    HAVE_X11=yes
    DISPLAY_ENV+=("DISPLAY=:0")
    DISPLAY_NOTE="${DISPLAY_NOTE:+$DISPLAY_NOTE, and }the X socket :0 (the shell does not export DISPLAY)"
  fi
  [ -n "$DISPLAY_NOTE" ]
}

display_fix() {
  if ! is_wsl; then
    fix "run the probe where a display exists: a desktop session, or for a test: xvfb-run -a nix run .#wsl2-probe"
  elif [ -d /mnt/wslg ]; then
    fix "WSLg is there, but this shell does not know it. A login shell that wsl.exe starts has these set;" \
      "here: export XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir WAYLAND_DISPLAY=wayland-0 DISPLAY=:0" \
      "(start the shell with wsl.exe -d $DISTRO, not through su or ssh)"
  else
    fix "WSLg is not running: update WSL (PowerShell: wsl --update, then wsl --shutdown)," \
      ".wslconfig [wsl2] must not have guiApplications=false; WSLg needs Windows 11 or a recent Windows 10"
  fi
}

# window_run ENV... -- QEMU-DISPLAY-ARGS...: opens the guest in a window for HOLD seconds, and
# sets WIN_OK (yes or no) and WIN_WHY
window_run() {
  local -a venv=() accels=(tcg)
  while [ $# -gt 0 ] && [ "$1" != -- ]; do
    venv+=("$1")
    shift
  done
  shift
  [ "$(kv_get kvm_usable)" = yes ] && accels=(kvm tcg)
  qemu_args hold "${accels[@]}" -- "$@"
  run_guest $((HOLD + 120)) "${venv[@]}" -- "${QARGS[@]}"
  WIN_OK=no
  if [ "$RG_RC" = 0 ] && grep -q '^PROBE:DONE' "$RG_SERIAL"; then
    WIN_OK=yes
    WIN_WHY="QEMU started, the guest ran $HOLD s with its window up and powered off by itself (exit 0, $((RG_MS / 1000)) s in all)"
  elif [ "$RG_RC" = 124 ]; then
    WIN_WHY="QEMU stayed up, but the guest did not finish in $((HOLD + 120)) s"
  else
    WIN_WHY="QEMU stopped ($(rc_text "$RG_RC")) before the guest finished"
  fi
}

# the lines of QEMU's error that say there is no display
no_display_error() {
  grep -qiE 'gtk initialization failed|cannot open display|could not open display|display .* is not available|no display|unable to init|failed to open display' "$RG_ERR" 2>/dev/null
}

p2_display() {
  heading "P2 display"
  fact P2 environment "WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-unset} DISPLAY=${DISPLAY:-unset} XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-unset}"
  fact P2 wslg "/mnt/wslg $([ -d /mnt/wslg ] && echo present || echo absent); runtime-dir: $(list_dir /mnt/wslg/runtime-dir | tr '\n' ' '); /tmp/.X11-unix: $(list_dir /tmp/.X11-unix | tr '\n' ' ')"

  if resolve_display; then
    if [ ${#DISPLAY_ENV[@]} -eq 0 ]; then
      verdict P2.1 PASS "a display is reachable: $DISPLAY_NOTE"
    else
      verdict P2.1 WARN "a display is reachable only through the defaults of WSLg: $DISPLAY_NOTE"
      display_fix
    fi
  elif is_wsl; then
    verdict P2.1 FAIL "no display: no Wayland socket, no DISPLAY, no WSLg socket"
    display_fix
  else
    verdict P2.1 WARN "no display: this machine has none, a legitimate answer outside WSL; the window check runs and reports what QEMU says"
  fi

  if [ "$DRY" = 1 ]; then
    qemu_args hold kvm tcg -- -vga none -device "$VGA" -display gtk
    out "  would run (a window of $HOLD s): env ${DISPLAY_ENV[*]:-} $(printf '%q ' "${QARGS[@]}")"
    verdict P2.2 SKIP "dry run"
    return
  fi
  if ! guest_ready; then
    guest_missing P2.2
    return
  fi

  detail ">>> Look at the Windows desktop now: a QEMU window should open for about $HOLD seconds."
  window_run "${DISPLAY_ENV[@]}" -- -vga none -device "$VGA" -display gtk
  if [ "$WIN_OK" = no ] && [ "$HAVE_X11$HAVE_WAYLAND" = yesyes ]; then
    # the window system the toolkit picked failed: try each of the two by name
    local backend
    for backend in x11 wayland; do
      detail "retrying with GDK_BACKEND=$backend"
      window_run "${DISPLAY_ENV[@]}" "GDK_BACKEND=$backend" -- -vga none -device "$VGA" -display gtk
      if [ "$WIN_OK" = yes ]; then
        kv_set gdk_backend "$backend"
        break
      fi
    done
  fi
  if [ "$WIN_OK" = yes ]; then
    verdict P2.2 PASS "$WIN_WHY"
    ask_window P2.3 "the QEMU window"
  else
    if no_display_error; then
      verdict P2.2 "$(is_wsl && echo FAIL || echo WARN)" "QEMU has no display to open its window on: $WIN_WHY"
      display_fix
    else
      verdict P2.2 FAIL "$WIN_WHY"
    fi
    head_lines 8 "$RG_ERR" | block "       qemu: "
    [ -s "$RG_SERIAL" ] && detail "the guest's output: $(head_lines 3 "$RG_SERIAL" | tr '\n' ' ')"
  fi
  detail "frame rate of the guest's framebuffer: not measured (the guest is a text console)"
}

# ask_window ID WHAT: asks whether the user saw the window, and records the answer
ask_window() {
  local answer
  answer=$(ask_yes_no "Did $2 appear on the Windows desktop?")
  case $answer in
  yes) verdict "$1" PASS "you saw $2 on the Windows desktop" ;;
  "yes ("*) verdict "$1" INFO "$2 was not looked at: $answer" ;;
  no)
    verdict "$1" FAIL "QEMU ran, but you saw no window on the Windows desktop"
    fix "WSLg may be drawing on a display that is not in view: restart it (PowerShell: wsl --shutdown) and run again"
    ;;
  *) verdict "$1" INFO "you were not asked whether $2 appeared, or did not answer ($answer)" ;;
  esac
}

# ---------------------------------------------------------------------------------------------
# P3: GPU
# ---------------------------------------------------------------------------------------------

# variant_env NAME: sets VENV, the environment variables of an OpenGL variant
variant_env() {
  VENV=()
  case $1 in
  wsl-lib) VENV=("LD_LIBRARY_PATH=/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}") ;;
  d3d12) VENV=("GALLIUM_DRIVER=d3d12") ;;
  wsl-lib+d3d12) VENV=("GALLIUM_DRIVER=d3d12" "LD_LIBRARY_PATH=/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}") ;;
  esac
}

# renderer_class LINES: the best class among renderer lines: d3d12, hardware, software or none
renderer_class() {
  local best=none lower text
  while IFS= read -r text; do
    lower=${text,,}
    case $lower in
    *d3d12*) best=d3d12 ;;
    *llvmpipe* | *softpipe* | *swrast* | *"software rasterizer"*) [ "$best" = none ] && best=software ;;
    "") ;;
    *) [ "$best" != d3d12 ] && best=hardware ;;
    esac
  done <<<"$1"
  echo "$best"
}

# gl_renderers VARIANT: the renderers eglinfo and glxinfo report in the variant's environment
gl_renderers() {
  variant_env "$1"
  env "${DISPLAY_ENV[@]}" "${VENV[@]}" timeout 25 eglinfo -B >"$STATE/egl.$1" 2>&1 </dev/null
  awk '/platform:$/ { plat = $0; sub(/ platform:$/, "", plat) }
       /renderer( string)?:/ { r = $0; sub(/^[^:]*:[ \t]*/, "", r); k = plat " " r; if (!(k in seen)) { seen[k] = 1; print "eglinfo " k } }' \
    "$STATE/egl.$1"
  if [ "$HAVE_X11" = yes ] && have glxinfo; then
    env "${DISPLAY_ENV[@]}" "${VENV[@]}" timeout 25 glxinfo -B >"$STATE/glx.$1" 2>&1 </dev/null
    sed -n 's/^OpenGL renderer string: /glxinfo X11 /p' "$STATE/glx.$1" | head -n1
  fi
}

p3_gpu() {
  heading "P3 GPU"
  p3_files
  if [ "$DRY" = 1 ]; then
    out "  would run: eglinfo -B and glxinfo -B in four environments (the shell's; LD_LIBRARY_PATH=/usr/lib/wsl/lib;"
    out "             GALLIUM_DRIVER=d3d12; both), then $QEMU -device help and -display help,"
    qemu_args hold kvm tcg -- -vga none -device "$VGA_GL" -display gtk,gl=on
    out "             then the window: $(printf '%q ' "${QARGS[@]}")"
    verdict P3.4 SKIP "dry run"
    return
  fi
  resolve_display
  p3_renderers
  p3_qemu_gl
}

p3_files() {
  local wsl_libs="" name driver_libs gpu
  if [ -e /dev/dxg ]; then
    verdict P3.1 PASS "/dev/dxg exists ($(file_owner /dev/dxg)): WSL2 passes the Windows GPU through"
  elif is_wsl; then
    verdict P3.1 FAIL "/dev/dxg does not exist: the GPU is not passed through to this WSL2 machine"
    fix "update the Windows GPU driver to one with WSL support (WDDM 2.9 or newer: AMD Adrenalin, NVIDIA, Intel)," \
      "then in PowerShell: wsl --update ; wsl --shutdown   and reopen"
  else
    verdict P3.1 INFO "/dev/dxg does not exist: this is not WSL (DRM nodes: $(list_dir /dev/dri | tr '\n' ' '))"
  fi

  for name in libd3d12.so libd3d12core.so libdxcore.so; do
    [ -e "/usr/lib/wsl/lib/$name" ] && wsl_libs="$wsl_libs $name"
  done
  fact P3 usr_lib_wsl_lib "$(list_dir /usr/lib/wsl/lib | tr '\n' ' ')"
  fact P3 usr_lib_wsl_drivers "$(list_dir /usr/lib/wsl/drivers | head -n 8 | tr '\n' ' ')"
  if [[ " $wsl_libs " == *" libd3d12.so "* ]] && [[ " $wsl_libs " == *" libdxcore.so "* ]]; then
    verdict P3.2 PASS "the Windows driver libraries are in /usr/lib/wsl/lib:$wsl_libs"
  elif is_wsl; then
    verdict P3.2 WARN "the Windows driver libraries are not all in /usr/lib/wsl/lib (found:${wsl_libs:- none})"
    fix "update WSL and the Windows GPU driver; WSL mounts /usr/lib/wsl when it starts (wsl --update ; wsl --shutdown)"
  else
    verdict P3.2 INFO "there is no /usr/lib/wsl/lib: this is not WSL"
  fi

  driver_libs=$(find /run/opengl-driver/lib -maxdepth 1 \( -name 'libd3d12*' -o -name 'libdxcore*' \) 2>/dev/null | tr '\n' ' ')
  fact P3 run_opengl_driver "d3d12 libraries: ${driver_libs:-none}; libd3d12.so -> $(readlink /run/opengl-driver/lib/libd3d12.so 2>/dev/null); dri: $(list_dir /run/opengl-driver/lib/dri | head -n 6 | tr '\n' ' ')"
  fact P3 ld_library_path "${LD_LIBRARY_PATH:-unset}"
  if [ -n "$driver_libs" ]; then
    verdict P3.3 PASS "wsl.useWindowsDriver is in effect: the Windows libraries are linked into /run/opengl-driver/lib"
  elif is_wsl; then
    verdict P3.3 WARN "no Windows driver library in /run/opengl-driver/lib: wsl.useWindowsDriver does not seem to be on"
    fix "in the NixOS-WSL configuration:" \
      "  wsl.useWindowsDriver = true;" \
      "  hardware.graphics.enable = true;" \
      "then: sudo nixos-rebuild switch"
  else
    verdict P3.3 INFO "not WSL: wsl.useWindowsDriver does not apply"
  fi

  gpu=$(win powershell.exe -NoProfile -NonInteractive -Command "Get-CimInstance Win32_VideoController | ForEach-Object { \$_.Name + ' ' + \$_.DriverVersion }" | tr '\n' ';')
  case $gpu in
  "" | *"not recognized"* | *Exception*) ;;
  *) fact P3 windows_gpu "$gpu" ;;
  esac
}

p3_renderers() {
  local variant best="" renderers class
  if ! have eglinfo; then
    verdict P3.4 SKIP "eglinfo is not on PATH: start the script as nix run .#wsl2-probe"
    return
  fi
  step "asking eglinfo and glxinfo for the host's OpenGL renderer in four environments"
  for variant in default wsl-lib d3d12 wsl-lib+d3d12; do
    renderers=$(gl_renderers "$variant")
    if [ -z "$renderers" ]; then
      fact P3 "host_gl[$variant]" "no renderer: $(head_lines 2 "$STATE/egl.$variant" | tr '\n' ' ')"
      continue
    fi
    fact P3 "host_gl[$variant]" "$(printf '%s' "$renderers" | tr '\n' ';')"
    class=$(renderer_class "$renderers")
    if [ -z "$best" ] && { [ "$class" = d3d12 ] || [ "$class" = hardware ]; }; then
      best=$variant
    fi
  done
  kv_set gl_best_variant "$best"
  if [ -n "$best" ]; then
    verdict P3.4 PASS "the host's OpenGL gives a hardware renderer in the variant '$best'"
    if [ "$best" != default ]; then
      variant_env "$best"
      detail "the real guest has to start QEMU with this environment: ${VENV[*]}"
    fi
  else
    verdict P3.4 WARN "no variant gives a hardware renderer: OpenGL on the host is software (llvmpipe) or missing"
    fix "NixOS-WSL: wsl.useWindowsDriver = true; hardware.graphics.enable = true; (see P3.3)," \
      "and the Windows GPU driver must be current; the real guest can fall back to software (--gpu off)"
  fi
}

p3_qemu_gl() {
  local devices displays best backend tried=default
  if ! have "$QEMU"; then
    guest_missing P3.5
    return
  fi
  devices=$("$QEMU" -device help 2>&1 | grep -oE 'virtio-(vga|gpu)[a-z-]*gl[a-z-]*' | sort -u | tr '\n' ' ')
  displays=$("$QEMU" -display help 2>&1 | awk 'NR > 1 && NF == 0 { exit } NR > 1 { printf "%s ", $1 }')
  fact P3 qemu_gl_devices "${devices:-none}"
  fact P3 qemu_displays "$displays"
  if [[ " $devices " == *" $VGA_GL "* ]] && [[ " $displays " == *" gtk "* ]]; then
    verdict P3.5 PASS "this QEMU has $VGA_GL and the GTK display"
  else
    verdict P3.5 FAIL "this QEMU lacks $VGA_GL or the GTK display"
    fix "it is the QEMU of nixpkgs (qemu_kvm); say so when you paste this, there is nothing to repair here"
    return
  fi

  if ! guest_ready; then
    guest_missing P3.6
    return
  fi
  backend=$(kv_get gdk_backend)
  [ -n "$backend" ] && DISPLAY_ENV+=("GDK_BACKEND=$backend")
  detail ">>> Look at the Windows desktop now: a window should open for about $HOLD seconds, with VirGL (gl=on)."
  window_run "${DISPLAY_ENV[@]}" -- -vga none -device "$VGA_GL" -display gtk,gl=on
  best=$(kv_get gl_best_variant)
  if [ "$WIN_OK" = no ] && [ -n "$best" ] && [ "$best" != default ]; then
    detail "retrying with the environment of the variant '$best'"
    variant_env "$best"
    window_run "${DISPLAY_ENV[@]}" "${VENV[@]}" -- -vga none -device "$VGA_GL" -display gtk,gl=on
    tried=$best
  fi
  if [ "$WIN_OK" = yes ]; then
    verdict P3.6 PASS "$WIN_WHY, with $VGA_GL and gl=on (environment of the variant '$tried')"
    detail "the renderer inside the guest is not checked here: the real guest reports it"
    ask_window P3.7 "the VirGL window"
  else
    if no_display_error; then
      verdict P3.6 "$(is_wsl && echo FAIL || echo WARN)" "QEMU has no display for the GL window: $WIN_WHY"
      display_fix
    else
      verdict P3.6 FAIL "QEMU refused or lost the GL display: $WIN_WHY"
      if grep -qiE 'gl|egl|epoxy|opengl' "$RG_ERR" 2>/dev/null; then
        fix "QEMU could not get an OpenGL context from the host: P3.4 says which environment gives a hardware" \
          "renderer; without one the real guest has to run with --gpu off"
      fi
    fi
    head_lines 8 "$RG_ERR" | block "       qemu: "
  fi
}

# ---------------------------------------------------------------------------------------------
# P4: the Windows side
# ---------------------------------------------------------------------------------------------

# free_port: a TCP port above 20000 that nothing listens on
free_port() {
  local port
  for _ in $(seq 1 50); do
    port=$((20000 + RANDOM % 30000))
    if ! ss -ltn "sport = :$port" 2>/dev/null | grep -q LISTEN; then
      echo "$port"
      return 0
    fi
  done
  return 1
}

# start_listener ADDRESS PORT: a socat that answers a line, until the script ends
start_listener() {
  socat "TCP-LISTEN:$2,bind=$1,reuseaddr,fork" "SYSTEM:echo wsl2-probe" >/dev/null 2>&1 &
  echo $! >>"$STATE/pids"
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    ss -ltn "sport = :$2" 2>/dev/null | grep -q LISTEN && return 0
    sleep 0.2
  done
  return 1
}

p4_windows() {
  heading "P4 Windows side"
  local ver mstsc cmdkey mode forwarding

  if [ "$DRY" = 1 ]; then
    out "  would run: cmd.exe /c ver; find mstsc.exe and cmdkey.exe; wslinfo --networking-mode;"
    out "             socat listeners on two free ports (127.0.0.1 and 0.0.0.0), then powershell.exe:"
    out "             a TCP connection from Windows to each, as 127.0.0.1 and as localhost"
    verdict P4.1 SKIP "dry run"
    return
  fi
  if ! is_wsl; then
    verdict P4.1 INFO "this is not WSL: there is no Windows side to check"
    return
  fi

  ver=$(win cmd.exe /c ver | grep -m1 Windows)
  if [ -n "$ver" ]; then
    verdict P4.1 PASS "Windows programs run from WSL (interop): $ver"
  else
    verdict P4.1 FAIL "cmd.exe does not run from WSL: interop is off or cmd.exe is not on PATH"
    fix "in the NixOS-WSL configuration:" \
      "  wsl.interop.includePath = true;" \
      "  wsl.interop.register = true;" \
      "  wsl.wslConf.interop.enabled = true;" \
      "then: sudo nixos-rebuild switch; in PowerShell: wsl --terminate $DISTRO, and reopen"
    return
  fi

  if mstsc=$(find_win mstsc.exe); then
    verdict P4.2 PASS "mstsc.exe (the Windows RDP client) is reachable: $mstsc"
  else
    verdict P4.2 WARN "mstsc.exe is neither on PATH nor in /mnt/c/Windows/System32"
    fix "the Remote Desktop client is part of Windows; the Windows PATH may be missing here: wsl.interop.includePath = true;"
  fi
  if cmdkey=$(find_win cmdkey.exe); then
    fact P4 cmdkey "$cmdkey"
  else
    fact P4 cmdkey "not found: the RDP credentials would have to be typed"
  fi

  mode=$(wslinfo --networking-mode 2>/dev/null)
  [ -n "$mode" ] || mode=$(wslconfig_get networkingMode)
  forwarding=$(wslconfig_get localhostForwarding)
  fact P4 networking_mode "${mode:-unknown (NAT is the default)}; localhostForwarding ${forwarding:-unset (true by default)}"
  fact P4 wsl_addresses "$(ip -brief addr 2>/dev/null | grep -v '^lo ' | tr -s ' ' | tr '\n' ';')"
  p4_forward
}

p4_forward() {
  local port_a port_b script lines ip_a name_a ip_b
  if ! have socat; then
    verdict P4.3 SKIP "socat is not on PATH: start the script as nix run .#wsl2-probe"
    return
  fi
  if ! { port_a=$(free_port) && port_b=$(free_port); }; then
    verdict P4.3 SKIP "no free high port found"
    return
  fi
  start_listener 127.0.0.1 "$port_a"
  start_listener 0.0.0.0 "$port_b"

  # one line: a script with newlines in it does not survive every way a command line reaches
  # powershell.exe
  script="foreach (\$t in @(@('127.0.0.1', $port_a), @('localhost', $port_a), @('127.0.0.1', $port_b), @('localhost', $port_b))) {"
  script="$script \$r = 'closed'; try { \$c = New-Object Net.Sockets.TcpClient;"
  script="$script \$w = \$c.BeginConnect(\$t[0], \$t[1], \$null, \$null);"
  script="$script if (\$w.AsyncWaitHandle.WaitOne(3000) -and \$c.Connected) { \$r = 'open' }; \$c.Close() }"
  script="$script catch { \$r = 'error ' + \$_.Exception.Message };"
  script="$script \$t[0] + ':' + \$t[1] + ' ' + \$r }"
  step "connecting from Windows (powershell.exe starts slowly)"
  lines=$(WIN_TIMEOUT=60 win powershell.exe -NoProfile -NonInteractive -Command "$script")
  if [ -z "$lines" ]; then
    verdict P4.3 WARN "powershell.exe gave no answer, so the forward was not tested"
    fix "powershell.exe may be blocked or missing. By hand: in WSL, socat TCP-LISTEN:3390,bind=127.0.0.1,fork -" \
      "and in PowerShell: Test-NetConnection 127.0.0.1 -Port 3390"
    return
  fi
  detail "from Windows (powershell.exe, a TCP connect with a 3 s limit): a listener on 127.0.0.1:$port_a and one on 0.0.0.0:$port_b"
  printf '%s\n' "$lines" | block "         "
  ip_a=$(printf '%s\n' "$lines" | sed -n "s/^127.0.0.1:$port_a //p")
  name_a=$(printf '%s\n' "$lines" | sed -n "s/^localhost:$port_a //p")
  ip_b=$(printf '%s\n' "$lines" | sed -n "s/^127.0.0.1:$port_b //p")
  fact P4 localhost_forward "listener on 127.0.0.1 reached as 127.0.0.1: ${ip_a:-?}, as localhost: ${name_a:-?}; listener on 0.0.0.0 reached as 127.0.0.1: ${ip_b:-?}"

  if [ "$ip_a" = open ] && [ "$name_a" = open ]; then
    verdict P4.3 PASS "Windows reaches a port WSL listens on at 127.0.0.1, as 127.0.0.1 and as localhost: mstsc /v:localhost:PORT can work"
  elif [ "$ip_a" = open ]; then
    verdict P4.3 WARN "Windows reaches 127.0.0.1 but not localhost (which may try ::1 first)"
    fix "use mstsc.exe /v:127.0.0.1:PORT, as the real command will"
  elif [ "$ip_b" = open ]; then
    verdict P4.3 WARN "Windows reaches a listener on 0.0.0.0 but not one on 127.0.0.1"
    fix "the guest's port would have to be published on 0.0.0.0, and not only on 127.0.0.1; say so when you paste this"
  else
    verdict P4.3 FAIL "Windows does not reach a port that WSL listens on"
    fix "in $(wslconfig_name):" \
      "  [wsl2]" \
      "  localhostForwarding=true" \
      "  networkingMode=nat           (mirrored mode shares the loopback instead: try the other mode)" \
      "then in PowerShell: wsl --shutdown, reopen, and run the probe again"
  fi
}

# ---------------------------------------------------------------------------------------------
# P5: resources
# ---------------------------------------------------------------------------------------------

p5_resources() {
  heading "P5 resources for the real guest"
  detail "the guest wants 8 GiB of memory, 4 or more cores and about 25 GiB of disk, plus the Nix build"
  local kib cpus free
  kib=$(mem_kib MemTotal)
  cpus=$(nproc)
  free=$(disk_free_kib)

  # 8 GiB for the guest, 2 GiB for WSL and Nix next to it
  if [ "${kib:-0}" -ge $((10 * 1048576)) ]; then
    verdict P5.1 PASS "memory: $(gib "$kib") GiB in this machine, 10 GiB wanted (8 for the guest, 2 for WSL and Nix)"
  elif [ "${kib:-0}" -ge $((6 * 1048576)) ]; then
    verdict P5.1 WARN "memory: $(gib "$kib") GiB in this machine, 10 GiB wanted: the guest would have to run with 6 GiB (--memory 6144)"
    p5_memory_fix
  else
    verdict P5.1 FAIL "memory: $(gib "${kib:-0}") GiB in this machine, less than the 6 GiB that is the least the guest can run with"
    p5_memory_fix
  fi

  if [ "$cpus" -ge 4 ]; then
    verdict P5.2 PASS "cores: $cpus in this machine, 4 wanted"
  else
    verdict P5.2 WARN "cores: $cpus in this machine, 4 or more wanted: the guest and the build would be slow"
    fix "in $(wslconfig_name) (now: processors=$(wslconfig_get processors || true)):" \
      "  [wsl2]" \
      "  processors=6" \
      "then in PowerShell: wsl --shutdown, and reopen"
  fi

  # 25 GiB for the Plasma closure and the guest's disk, 10 GiB for the build
  if [ -z "$free" ]; then
    verdict P5.3 SKIP "disk: the free space could not be read"
  elif [ "$free" -ge $((40 * 1048576)) ]; then
    verdict P5.3 PASS "disk: $(gib "$free") GiB free for the Nix store and the guest (under WSL: the Windows drive under the virtual disk), 40 GiB wanted"
  elif [ "$free" -ge $((25 * 1048576)) ]; then
    verdict P5.3 WARN "disk: $(gib "$free") GiB free, 40 GiB wanted (25 for the closure and the guest's disk, 10 for the build): it will be tight"
    p5_disk_fix
  else
    verdict P5.3 FAIL "disk: $(gib "$free") GiB free, 25 GiB is the least the closure and the guest's disk need"
    p5_disk_fix
  fi
}

p5_memory_fix() {
  fix "in $(wslconfig_name) (now: memory=$(wslconfig_get memory || true)):" \
    "  [wsl2]" \
    "  memory=12GB" \
    "  swap=8GB" \
    "then in PowerShell: wsl --shutdown, and reopen (the Windows machine needs about 16 GB for this)"
}

p5_disk_fix() {
  fix "free space on the Windows drive that holds the distribution's virtual disk (C: unless it was moved)," \
    "and in the distribution: nix-collect-garbage -d" \
    "the virtual disk can be grown too (PowerShell): wsl --manage $DISTRO --resize 200GB"
}

# ---------------------------------------------------------------------------------------------
# P6: virtiofs
# ---------------------------------------------------------------------------------------------

# The real guest mounts the host's /nix/store and its shared directories by virtiofs (nixpkgs does
# so for every Linux host). For each of them the guest's runner starts a virtiofsd, which serves a
# directory over a socket, and QEMU joins the guest's virtio-fs device to that socket (vhost-user)
# with the guest's memory in a shared memfd. This probe builds the same chain with the same
# options, for the host's /nix/store (read-only) and a temporary directory (writable).

# the options of nixpkgs' runner (nixos/modules/virtualisation/qemu-vm.nix), after the socket and
# the directory; the runner's --cache is always for the store and auto for the other shares
P6_VIRTIOFSD_COMMON=(--sandbox=none --seccomp=none --no-announce-submounts
  --translate-uid=host:65534:0:1 --translate-gid=host:65534:0:1)

# p6_daemon NAME DIR ro|rw CACHE: starts virtiofsd for the directory and waits for its socket;
# fails when it does not come up, with the daemon's output in $STATE/virtiofsd.NAME.log
p6_daemon() {
  local name=$1 dir=$2 access=$3 cache=$4 mode=--writeback pid
  [ "$access" = ro ] && mode=--readonly
  virtiofsd --socket-path="$STATE/virtiofsd.$name.sock" --shared-dir="$dir" "$mode" \
    --cache="$cache" "${P6_VIRTIOFSD_COMMON[@]}" >"$STATE/virtiofsd.$name.log" 2>&1 </dev/null &
  pid=$!
  echo "$pid" >>"$STATE/pids"
  kv_set "virtiofsd_pid_$name" "$pid"
  for _ in $(seq 1 50); do
    [ -S "$STATE/virtiofsd.$name.sock" ] && return 0
    kill -0 "$pid" 2>/dev/null || return 1
    sleep 0.1
  done
  return 1
}

p6_stop_daemons() {
  local name pid
  for name in store share; do
    pid=$(kv_get "virtiofsd_pid_$name")
    [ -n "$pid" ] && kill "$pid" 2>/dev/null
  done
  return 0
}

p6_fuse() {
  if [ ! -e /dev/fuse ]; then
    verdict P6.1 WARN "/dev/fuse does not exist: this kernel has no FUSE, or its device node was not made"
    detail "virtiofsd does not open /dev/fuse (it speaks FUSE to the guest over a socket), so the guest can still start;" \
      "what needs it is another FUSE program, not run-kde"
    fix "NixOS: boot.kernelModules = [ \"fuse\" ];  then sudo nixos-rebuild switch" \
      "WSL2: the kernel of WSL has FUSE built in (CONFIG_FUSE_FS=y in its 6.6 configuration), so a kernel= line" \
      "in $(wslconfig_name) [wsl2] that names another kernel is the likely cause"
  elif (exec 9<>/dev/fuse) 2>/dev/null; then
    verdict P6.1 PASS "/dev/fuse can be opened read-write by $ME ($(file_owner /dev/fuse)); virtiofsd does not need it"
  else
    verdict P6.1 WARN "/dev/fuse exists but $ME cannot open it ($(file_owner /dev/fuse)); virtiofsd does not need it"
    fix "NixOS: services.udev.extraRules = ''KERNEL==\"fuse\", MODE=\"0666\"'';  then sudo nixos-rebuild switch" \
      "for now: sudo chmod 666 /dev/fuse"
  fi
}

p6_virtiofs() {
  heading "P6 virtiofs (the guest's /nix/store and shared directories)"
  local resolved storedir share sha_host accels=(tcg) item seen=""
  p6_fuse

  if [ "$DRY" = 1 ]; then
    out "  would run: virtiofsd ${P6_VIRTIOFSD_COMMON[*]} --readonly --cache=always for /nix/store, and --writeback for a temporary directory,"
    out "             then $QEMU with -object memory-backend-memfd,share=on, -machine memory-backend=, and a vhost-user-fs-pci device for each;"
    out "             the guest loads virtio_pci and virtiofs, mounts both, reads and writes, and powers off"
    verdict P6.2 SKIP "dry run"
    return
  fi
  if ! have virtiofsd; then
    verdict P6.2 SKIP "virtiofsd is not on PATH: start the script as nix run .#wsl2-probe"
    return
  fi
  fact P6 virtiofsd "$(command -v virtiofsd), $(virtiofsd --version 2>&1 | head -n1)"
  if ! guest_ready; then
    guest_missing P6.2
    return
  fi
  if ! "$QEMU" -device help 2>&1 | grep -q 'vhost-user-fs-pci'; then
    verdict P6.2 FAIL "this QEMU has no vhost-user-fs-pci device, which the guest's /nix/store needs"
    fix "it is the QEMU of nixpkgs (qemu_kvm); say so when you paste this, there is nothing to repair here"
    return
  fi

  resolved=$(readlink -f "$GUEST/busybox")
  storedir=$(dirname "$(dirname "$(dirname "$resolved")")")
  P6_STOREFILE=${resolved#"$storedir"/}
  sha_host=$(sha256sum "$resolved" | cut -d' ' -f1)
  P6_TOKEN=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
  share=$STATE/p6-share
  mkdir -p "$share"
  printf '%s\n' "$P6_TOKEN" >"$share/token"

  if p6_daemon store "$storedir" ro always && p6_daemon share "$share" rw auto; then
    verdict P6.2 PASS "virtiofsd started as $ME (uid $(id -u)) and made its sockets: the store at $storedir read-only, a temporary directory writable"
    fact P6 virtiofsd_log "$(grep -h -E 'WARN|ERROR' "$STATE"/virtiofsd.*.log | sed 's/^\[[^ ]* *\([A-Z]*\) *\([^]]*\)\] /\1 \2: /' | sort -u | tr '\n' ';')"
  else
    verdict P6.2 FAIL "virtiofsd did not start or did not make its socket"
    head_lines 8 "$STATE/virtiofsd.store.log" | block "       virtiofsd: "
    head_lines 8 "$STATE/virtiofsd.share.log" | block "       virtiofsd: "
    fix "virtiofsd ran with the options of the guest's runner and could not serve a directory: the lines above say why." \
      "A socket path is 107 bytes at most, so a long \$TMPDIR is one cause: TMPDIR=/tmp nix run .#wsl2-probe" \
      "Say what virtiofsd printed when you paste this: the guest cannot start without it"
    p6_stop_daemons
    return
  fi

  [ -e /dev/kvm ] && (exec 9<>/dev/kvm) 2>/dev/null && accels=(kvm tcg)
  step "booting the guest with its /nix/store and a shared directory by virtiofs"
  qemu_args virtiofs "${accels[@]}" -- -display none \
    -object memory-backend-memfd,id=mem0,size=512M,share=on -machine memory-backend=mem0 \
    -chardev "socket,id=store,path=$STATE/virtiofsd.store.sock" -device vhost-user-fs-pci,chardev=store,tag=probestore \
    -chardev "socket,id=share,path=$STATE/virtiofsd.share.sock" -device vhost-user-fs-pci,chardev=share,tag=probe
  run_guest 300 -- "${QARGS[@]}"
  p6_stop_daemons

  if [ "$RG_RC" != 0 ] || ! grep -q '^PROBE:DONE' "$RG_SERIAL"; then
    verdict P6.3 FAIL "the guest with virtio-fs devices did not run to its end ($(rc_text "$RG_RC"))"
    head_lines 8 "$RG_ERR" | block "       qemu: "
    [ -s "$RG_SERIAL" ] && detail "the guest's output: $(head_lines 6 "$RG_SERIAL" | tr '\n' ' ')"
    if grep -qiE 'memory-backend|share|vhost' "$RG_ERR" 2>/dev/null; then
      fix "QEMU refused the shared memory or the vhost-user socket: say what it printed when you paste this"
    else
      fix "say what QEMU and the guest printed when you paste this"
    fi
    return
  fi
  verdict P6.3 PASS "QEMU started the guest with a shared memfd and two vhost-user-fs devices, on ${accels[0]}; boot to power-off $RG_MS ms"
  for item in virtio_pci virtiofs; do
    seen="$seen$item:$(guest_value "$RG_SERIAL" "MODULE_$item") "
  done
  fact P6 guest_modules "$seen"
  fact P6 guest_filesystems "$(guest_value "$RG_SERIAL" FILESYSTEMS)"

  p6_check_store "$sha_host"
  p6_check_share
}

# p6_why MOUNT: what the guest said about a mount, without its "failed: " in front
p6_why() {
  local why=${1#failed: }
  echo "${why:-the guest said nothing}"
}

# p6_check_store SHA: the guest read the host's /nix/store through virtiofs
p6_check_store() {
  local mount sha write
  mount=$(guest_value "$RG_SERIAL" STORE_MOUNT)
  sha=$(guest_value "$RG_SERIAL" STORE_SHA256)
  write=$(guest_value "$RG_SERIAL" STORE_WRITE)
  fact P6 guest_store "mount ${mount:-no answer}; sha256 of $P6_STOREFILE ${sha:-none}; write ${write:-no answer}"
  if [ "$mount" != ok ]; then
    verdict P6.4 FAIL "the guest could not mount the host's /nix/store by virtiofs ($(p6_why "$mount"))"
    p6_guest_fix
  elif [ "$sha" != "$1" ]; then
    verdict P6.4 FAIL "the guest mounted /nix/store but read another file than the host has (sha256 ${sha:-none}, expected $1)"
  elif [ "$write" != refused ]; then
    verdict P6.4 WARN "the guest read /nix/store by virtiofs, but could write to it ($write): the share is meant to be read-only"
  else
    verdict P6.4 PASS "the guest mounted the host's /nix/store by virtiofs read-only and read $P6_STOREFILE with the host's checksum"
  fi
}

# p6_check_share: the guest read and wrote a writable shared directory
p6_check_share() {
  local mount got write host_text
  mount=$(guest_value "$RG_SERIAL" SHARE_MOUNT)
  got=$(guest_value "$RG_SERIAL" SHARE_READ)
  write=$(guest_value "$RG_SERIAL" SHARE_WRITE)
  host_text=$(head -n1 "$STATE/p6-share/from-guest" 2>/dev/null)
  fact P6 guest_share "mount ${mount:-no answer}; listing $(guest_value "$RG_SERIAL" SHARE_LISTING); the host user's file seen as $(guest_value "$RG_SERIAL" SHARE_READ_OWNER); the guest's file seen as $(guest_value "$RG_SERIAL" SHARE_WRITE_OWNER)"
  fact P6 host_share "the guest's file on the host: $(file_owner "$STATE/p6-share/from-guest" || echo none) (host user $ME, uid $(id -u))"
  if [ "$mount" != ok ]; then
    verdict P6.5 FAIL "the guest could not mount a writable shared directory by virtiofs ($(p6_why "$mount"))"
    p6_guest_fix
  elif [ "$got" != "$P6_TOKEN" ]; then
    verdict P6.5 FAIL "the guest read '${got:-nothing}' from the shared directory, not the token the host wrote"
  elif [ "$write" != ok ] || [ "$host_text" != "$P6_TOKEN" ]; then
    verdict P6.5 FAIL "the guest could not write the shared directory, or the host did not see what it wrote (write: ${write:-no answer}; host read: ${host_text:-nothing})"
  else
    verdict P6.5 PASS "the guest read the host's file and wrote a file that the host reads, through a writable virtiofs share"
  fi
}

p6_guest_fix() {
  if [ "$(guest_value "$RG_SERIAL" MODULE_virtiofs)" != loaded ]; then
    fix "the guest kernel could not load virtiofs: $(guest_value "$RG_SERIAL" MODULE_virtiofs)" \
      "(a kernel of nixpkgs has it as a module; say so when you paste this)"
  else
    fix "virtiofsd ran and the guest has the module, but the mount failed: look at the virtiofsd lines (P6 virtiofsd_log)" \
      "and say what the guest printed when you paste this"
  fi
}

# ---------------------------------------------------------------------------------------------
# the dry run, the summary and the main loop
# ---------------------------------------------------------------------------------------------

dry_guest_plan() {
  detail "dry run: nothing is started. The guest would be:"
  qemu_args bench kvm -- -display none
  out "         under KVM: $(printf '%q ' "${QARGS[@]}")"
  qemu_args bench tcg -- -display none
  out "         under TCG: $(printf '%q ' "${QARGS[@]}")"
  out "         and, on this machine: busybox sh -c '. loop.sh; probe_loop $LOOPS'"
  verdict P1.3 SKIP "dry run"
}

# the worst status among the checks of one probe
probe_status() {
  awk -F'|' -v prefix="$1." '
    index($1, prefix) == 1 {
      if ($2 == "FAIL") fail = 1
      else if ($2 == "WARN") warn = 1
      else if ($2 == "PASS") pass = 1
      else if ($2 == "SKIP") skip = 1
    }
    END { print fail ? "FAIL" : warn ? "WARN" : pass ? "PASS" : skip ? "SKIP" : "INFO" }' "$STATE/verdicts"
}

summary() {
  local probe title text
  out ""
  out "================================ PASTE THIS ================================"
  out "komorebi (KDE Plasma) wsl2-probe, revision $REV, $(date -u +%Y-%m-%dT%H:%M:%SZ), $(uname -m)$([ "$DRY" = 1 ] && echo ', DRY RUN')"
  for probe in P0 P1 P2 P3 P4 P5 P6; do
    [ "$probe" = P0 ] || want "$probe" || continue
    case $probe in
    P0) title="environment" ;;
    P1) title="KVM" ;;
    P2) title="display (a QEMU window through WSLg)" ;;
    P3) title="GPU (VirGL)" ;;
    P4) title="Windows side (mstsc, the localhost forward)" ;;
    P5) title="resources for the real guest" ;;
    P6) title="virtiofs (the guest's /nix/store and shares)" ;;
    esac
    out "$probe $title: $(probe_status "$probe")"
    awk -F'|' -v prefix="$probe." 'index($1, prefix) == 1 { printf "  %s %s %s\n", $1, $2, $3 }' "$STATE/verdicts" | block ""
  done
  out "-- facts"
  while IFS= read -r text; do
    out "$text"
  done <"$STATE/facts"
  out "============================== END OF PASTE ================================"
  out ""
  out "The report is in $REPORT_PATH"
  out "Paste the block between the two lines of equals signs; the whole report if it is asked for."
}

out "wsl2-probe of komorebi (KDE Plasma), revision $REV: $(date -u +%Y-%m-%dT%H:%M:%SZ)$([ "$DRY" = 1 ] && echo ' (dry run)')"
out "It changes nothing: it reads the machine, boots a tiny guest that powers itself off, and listens on local ports for a few seconds."
if [ "$DRY" = 0 ] && { want P2 || want P3; }; then
  out "It takes about two minutes. A window opens for $HOLD seconds in P2 and in P3: look at the Windows desktop."
fi

# each probe runs in a subshell of its own: one that dies leaves the others alone, and the file
# it writes at the end says that it did not die
run_probe() {
  local probe=$1
  [ "$probe" = P0 ] || want "$probe" || return 0
  case $probe in
  P0) (p0_environment; : >"$STATE/done.P0") ;;
  P1) (p1_kvm; : >"$STATE/done.P1") ;;
  P2) (p2_display; : >"$STATE/done.P2") ;;
  P3) (p3_gpu; : >"$STATE/done.P3") ;;
  P4) (p4_windows; : >"$STATE/done.P4") ;;
  P5) (p5_resources; : >"$STATE/done.P5") ;;
  P6) (p6_virtiofs; : >"$STATE/done.P6") ;;
  esac
  if [ ! -e "$STATE/done.$probe" ]; then
    verdict "$probe.99" FAIL "the probe stopped before it finished (a bug in the probe): the lines above are what it found"
  fi
}

for probe in P0 P1 P2 P3 P4 P5 P6; do
  run_probe "$probe"
done

summary
exit 0
