#!/usr/bin/env bash
# Probe only: profiles of single rows of the Rust harness, each run over and
# over so that it fills the profile, with the callers of the hottest
# functions.
#
#   profile.sh OUT STORE:ROW:REPEAT...
set -euo pipefail

out=$1
shift
bin=bench/rust/target/release/darudb-bench

mkdir -p "$out"
lscpu | grep -E 'Model name|^CPU\(s\)' > "$out/machine.txt"

for focus in "$@"; do
  IFS=: read -r store row repeat <<< "$focus"
  name="$store-$row"
  dir=$(mktemp -d)
  BENCH_SKIP=insert-sync,insert-deferred BENCH_FOCUS=$row BENCH_REPEAT=$repeat perf record -q -e cpu-clock -F 1999 \
    --call-graph dwarf,16384 -o "perf-$name.data" "$bin" --child "$store" --dir "$dir" > "$out/rows-$name.txt"
  rm -rf "$dir"
  {
    echo "######## $name: by object"
    perf report -i "perf-$name.data" --no-children --sort dso --stdio -g none 2> /dev/null | grep -v '^#' | grep -v '^$' | head -6
    echo "######## $name: by function, own time"
    perf report -i "perf-$name.data" --no-children --sort symbol --stdio -g none --percent-limit 0.7 2> /dev/null | grep -v '^#' | grep -v '^$' | head -50
    echo "######## $name: callers of the hottest functions"
    perf report -i "perf-$name.data" --no-children --sort symbol --stdio -G --percent-limit 3 2> /dev/null | grep -v '^#' | grep -v '^$' | head -400
  } > "$out/profile-$name.txt"
done
