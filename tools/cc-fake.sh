#!/usr/bin/env bash
# check-only hack: emit an empty object for every compile so C -sys build scripts "succeed"
out=""; prev=""; compile=0
for a in "$@"; do
  [ "$prev" = "-o" ] && out="$a"
  [ "$a" = "-c" ] && compile=1
  [ "$a" = "-E" ] && { exec gcc "${@}" ; }
  case "$a" in -Fo*) out="${a#-Fo}";; esac
  prev="$a"
done
if [ $compile = 1 ] && [ -n "$out" ]; then exec gcc -x c -c /dev/null -o "$out"; fi
exit 0
