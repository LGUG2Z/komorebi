# A CPU loop that scripts/wsl2-probe.sh times in three places: on the host, in a guest under
# KVM and in a guest under TCG. busybox's ash runs it, so the same interpreter runs the same
# instructions everywhere and only the way the CPU is provided differs.
#
# `probe_loop N` counts to N and prints the milliseconds it took, read from /proc/uptime, whose
# resolution is 10 ms: a loop shorter than about a second says little.

# the uptime in centiseconds; the 1 in front of the fraction keeps ash from reading 08 as octal
probe_centis() {
  read -r up _ </proc/uptime
  echo $((${up%.*} * 100 + 1${up#*.} - 100))
}

probe_loop() {
  start=$(probe_centis)
  i=0
  while [ "$i" -lt "$1" ]; do
    i=$((i + 1))
  done
  end=$(probe_centis)
  echo $(((end - start) * 10))
}
