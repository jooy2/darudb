#!/usr/bin/env bash
# Probe only: profiles of single rows of the Rust harness, each run over and
# over so that it fills the profile, with the callers of the hottest
# functions.
#
#   profile.sh OUT STORE:ROW:REPEAT[:FREQUENCY]...
#
# A row that inserts cannot repeat, since its keys would be taken; a higher
# sampling frequency fills its profile instead.
set -euo pipefail

out=$1
shift
bin=bench/rust/target/release/darudb-bench
rows="insert-sync insert-deferred insert-bulk get-key get-email age-equal age-range count city-scan top-score update delete"

mkdir -p "$out"
lscpu | grep -E 'Model name|^CPU\(s\)' > "$out/machine.txt"

for focus in "$@"; do
  IFS=: read -r store row repeat freq <<< "$focus"
  name="$store-$row"
  dir=$(mktemp -d)
  # Every row before the focused one but the bulk insert, which the reads
  # need, is left out.
  skip=$(printf '%s\n' $rows | grep -vx -e insert-bulk -e "$row" | paste -sd, -)
  BENCH_SKIP=$skip BENCH_FOCUS=$row BENCH_REPEAT=$repeat perf record -q -e cpu-clock -F "${freq:-1999}" \
    --call-graph dwarf,16384 -o "perf-$name.data" "$bin" --child "$store" --dir "$dir" > "$out/rows-$name.txt"
  rm -rf "$dir"
  {
    echo "######## $name: by object"
    perf report -i "perf-$name.data" --no-children --sort dso --stdio -g none 2> /dev/null | grep -v '^#' | grep -v '^$' | head -6
    echo "######## $name: by function, own time"
    perf report -i "perf-$name.data" --no-children --sort symbol --stdio -g none --percent-limit 0.7 2> /dev/null | grep -v '^#' | grep -v '^$' | head -50
    echo "######## $name: system calls, time with callees"
    perf report -i "perf-$name.data" --children --sort symbol --stdio -g none 2> /dev/null | grep -E '\[k\] (__x64_sys_|__do_sys_|ksys_|do_fcntl|fcntl_)' | head -20
    echo "######## $name: own code, time with callees"
    perf report -i "perf-$name.data" --children --sort symbol --stdio -g none --percent-limit 2 2> /dev/null | grep -E '\[\.\] (darudb|<darudb)' | head -40
    echo "######## $name: callers of the hottest functions"
    perf report -i "perf-$name.data" --no-children --sort symbol --stdio -G --percent-limit 3 2> /dev/null | grep -v '^#' | grep -v '^$' | head -400
  } > "$out/profile-$name.txt"
done
